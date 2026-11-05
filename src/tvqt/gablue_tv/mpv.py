"""Lecteur vidéo embarqué : libmpv + OpenGL (`mpv_render_context`).

La détection de VM et le chargement de libmpv se font à l'import : les
variables d'environnement de rendu logiciel doivent être posées avant toute
création de contexte OpenGL.
"""

import ctypes
import locale
import os
from ctypes import c_char_p, c_int, c_void_p, CFUNCTYPE, Structure

from PySide6.QtCore import Qt, Signal
from PySide6.QtGui import QCursor, QOpenGLContext
from PySide6.QtOpenGLWidgets import QOpenGLWidget
from PySide6.QtWidgets import QApplication

# --- Chargement libmpv ---
_LIBMPV_PATH = ""
for _candidate in ("/usr/lib64/libmpv.so.2", "/usr/lib64/libmpv.so.1",
                     "/usr/lib64/libmpv.so"):
    if os.path.exists(_candidate):
        _LIBMPV_PATH = _candidate
        break
if not _LIBMPV_PATH:
    raise RuntimeError("libmpv.so introuvable")

def _is_virtual_machine():
    """Détecte une VM (QEMU/KVM/virgl, etc.).

    En VM, le décodage matériel (VDPAU/virgl) échoue et fige le pipeline
    virtio-gpu ("got error from kernel - expect bad rendering"). On force
    alors le décodage logiciel.
    """
    try:
        with open("/sys/class/dmi/id/product_name") as f:
            product = f.read().strip().lower()
        if any(v in product for v in ("kvm", "qemu", "virtual", "vmware", "bochs")):
            return True
    except OSError:
        pass
    try:
        with open("/sys/class/dmi/id/sys_vendor") as f:
            vendor = f.read().strip().lower()
        if any(v in vendor for v in ("qemu", "red hat", "innotek", "vmware", "microsoft corporation")):
            return True
    except OSError:
        pass
    return False


_IS_VM = _is_virtual_machine()

# En VM, virgl (virtio-gpu) fuit des descripteurs de fences en rendu GL
# soutenu : au bout de ~15s la limite de fds est atteinte, l'ioctl EXECBUFFER
# renvoie EMFILE et le driver casse tout le contexte GL ("got error from
# kernel - expect bad rendering 24"), figeant vidéo + UI (l'audio, décodé sur
# un autre thread, continue). On bascule sur le rendu logiciel (llvmpipe) qui
# n'utilise pas virgl. Doit être défini avant toute création de contexte GL.
if _IS_VM:
    os.environ["LIBGL_ALWAYS_SOFTWARE"] = "1"
    os.environ["GALLIUM_DRIVER"] = "llvmpipe"

# Constantes mpv_render_param_type
_MPV_RENDER_PARAM_API_TYPE = 1
_MPV_RENDER_PARAM_OPENGL_INIT_PARAMS = 2
_MPV_RENDER_PARAM_OPENGL_FBO = 3
_MPV_RENDER_PARAM_FLIP_Y = 4
_MPV_RENDER_PARAM_INVALID = 0

class _MpvOpenGLInitParams(Structure):
    _fields_ = [("get_proc_address", c_void_p), ("get_proc_address_ctx", c_void_p)]


class _MpvOpenGLFBO(Structure):
    _fields_ = [("fbo", c_int), ("w", c_int), ("h", c_int), ("internal_format", c_int)]


class _MpvRenderParam(Structure):
    _fields_ = [("type", c_int), ("data", c_void_p)]


_GetProcAddrFn = CFUNCTYPE(c_void_p, c_void_p, c_char_p)
_UpdateCallbackFn = CFUNCTYPE(None, c_void_p)


class MpvWidget(QOpenGLWidget):
    """Widget vidéo embarquant mpv via libmpv et OpenGL."""

    # Signal émis depuis le thread mpv, reçu dans le thread GUI (QueuedConnection).
    # mpv appelle le callback de mise à jour depuis un thread interne : on ne
    # doit JAMAIS toucher à Qt directement depuis ce thread (deadlock/freeze,
    # visible surtout sous QEMU/virgl où le rendu GL est plus lent).
    _mpv_updated = Signal()

    def __init__(self, parent=None):
        super().__init__(parent)
        self.setUpdateBehavior(QOpenGLWidget.UpdateBehavior.PartialUpdate)
        self._mpv_updated.connect(self.update, Qt.QueuedConnection)

        locale.setlocale(locale.LC_NUMERIC, "C")
        self._lib = ctypes.CDLL(_LIBMPV_PATH)
        self._setup_libmpv_api()

        self._handle = self._lib_mpv_create()
        # Python 3.14 : c_void_p devient int, wrapper explicite requis
        self._handle = c_void_p(self._handle)
        if not self._handle:
            raise RuntimeError("libmpv: mpv_create() failed")

        self._lib_mpv_set_option_string(self._handle, b"vo", b"libmpv")
        self._lib_mpv_set_option_string(self._handle, b"gpu-api", b"opengl")
        # En VM : décodage logiciel (VDPAU/virgl échoue et fige virtio-gpu)
        self._lib_mpv_set_option_string(
            self._handle, b"hwdec", b"no" if _IS_VM else b"auto-safe"
        )
        self._lib_mpv_set_option_string(self._handle, b"profile", b"fast")
        self._lib_mpv_set_option_string(self._handle, b"alang", b"fr,eng,ita,deu,spa,jpn,und")
        self._lib_mpv_set_option_string(self._handle, b"slang", b"fr,eng")
        self._lib_mpv_set_option_string(self._handle, b"keep-open", b"yes")
        self._lib_mpv_set_option_string(self._handle, b"stop-screensaver", b"yes")

        if self._lib_mpv_initialize(self._handle) < 0:
            raise RuntimeError("libmpv: mpv_initialize() failed")

        self._gl_ctx = None
        self._get_proc_addr_cb = _GetProcAddrFn(self._get_proc_address)
        self._update_cb = _UpdateCallbackFn(self._on_mpv_update)
        self._playing = False
        self._right_click_cb = None

    def _setup_libmpv_api(self):
        L = self._lib
        self._lib_mpv_create = L.mpv_create
        self._lib_mpv_create.restype = c_void_p
        self._lib_mpv_initialize = L.mpv_initialize
        self._lib_mpv_initialize.argtypes = [c_void_p]
        self._lib_mpv_initialize.restype = c_int
        self._lib_mpv_set_option_string = L.mpv_set_option_string
        self._lib_mpv_set_option_string.argtypes = [c_void_p, c_char_p, c_char_p]
        self._lib_mpv_set_option_string.restype = c_int
        self._lib_mpv_command_string = L.mpv_command_string
        self._lib_mpv_command_string.argtypes = [c_void_p, c_char_p]
        self._lib_mpv_command_string.restype = c_int
        self._lib_mpv_get_property = L.mpv_get_property
        self._lib_mpv_get_property.restype = c_int
        self._lib_mpv_terminate_destroy = L.mpv_terminate_destroy
        self._lib_mpv_terminate_destroy.argtypes = [c_void_p]
        self._lib_mpv_render_context_create = L.mpv_render_context_create
        self._lib_mpv_render_context_create.restype = c_int
        self._lib_mpv_render_context_set_update_callback = L.mpv_render_context_set_update_callback
        self._lib_mpv_render_context_set_update_callback.argtypes = [c_void_p, _UpdateCallbackFn, c_void_p]
        self._lib_mpv_render_context_render = L.mpv_render_context_render
        self._lib_mpv_render_context_render.restype = c_int
        self._lib_mpv_render_context_free = L.mpv_render_context_free
        self._lib_mpv_render_context_free.argtypes = [c_void_p]

    @staticmethod
    def _get_proc_address(_ctx, name):
        """Callback de résolution des fonctions OpenGL pour libmpv."""
        glctx = QOpenGLContext.currentContext()
        if not glctx:
            return 0
        try:
            fn = name.decode() if isinstance(name, bytes) else str(name)
            addr = glctx.getProcAddress(fn)
            return int(addr) if addr else 0
        except Exception:
            return 0

    def initializeGL(self):
        init_params = _MpvOpenGLInitParams(
            ctypes.cast(self._get_proc_addr_cb, c_void_p),
            c_void_p(0),
        )

        params = (_MpvRenderParam * 4)()
        params[0] = _MpvRenderParam(
            _MPV_RENDER_PARAM_API_TYPE,
            ctypes.cast(ctypes.c_char_p(b"opengl"), c_void_p),
        )
        params[1] = _MpvRenderParam(
            _MPV_RENDER_PARAM_OPENGL_INIT_PARAMS,
            ctypes.cast(ctypes.byref(init_params), c_void_p),
        )
        params[2] = _MpvRenderParam(_MPV_RENDER_PARAM_INVALID, c_void_p(0))

        gl_ctx = c_void_p()
        if self._lib_mpv_render_context_create(
            ctypes.byref(gl_ctx), self._handle, params
        ) < 0:
            raise RuntimeError("libmpv: render_context_create() failed")
        self._gl_ctx = gl_ctx
        self._lib_mpv_render_context_set_update_callback(
            self._gl_ctx, self._update_cb, c_void_p(0)
        )

    def paintGL(self):
        if not self._gl_ctx:
            return
        rect = self.rect()
        fbo_data = _MpvOpenGLFBO(
            self.defaultFramebufferObject(), rect.width(), rect.height(), 0
        )
        flip_y = c_int(1)

        params = (_MpvRenderParam * 3)()
        params[0] = _MpvRenderParam(
            _MPV_RENDER_PARAM_OPENGL_FBO, ctypes.cast(ctypes.byref(fbo_data), c_void_p)
        )
        params[1] = _MpvRenderParam(
            _MPV_RENDER_PARAM_FLIP_Y, ctypes.cast(ctypes.byref(flip_y), c_void_p)
        )
        params[2] = _MpvRenderParam(_MPV_RENDER_PARAM_INVALID, c_void_p(0))

        self._lib_mpv_render_context_render(self._gl_ctx, params)

    def _on_mpv_update(self, _ctx):
        self._mpv_updated.emit()

    def play(self, url):
        self._lib_mpv_command_string(self._handle, f"loadfile {url}".encode())
        self._playing = True

    def is_rendering(self):
        """Retourne True si la vidéo est effectivement en train de jouer."""
        if not self._playing:
            return False
        val = ctypes.c_double()
        fmt = c_int(5)  # MPV_FORMAT_DOUBLE
        ret = self._lib_mpv_get_property(
            self._handle, b"time-pos", fmt, ctypes.byref(val)
        )
        return ret >= 0

    def command(self, *args):
        cmd = " ".join(str(a) for a in args).encode()
        self._lib_mpv_command_string(self._handle, cmd)

    def stop(self):
        if self._gl_ctx:
            self._lib_mpv_render_context_free(self._gl_ctx)
            self._gl_ctx = None
        if self._handle:
            self._lib_mpv_terminate_destroy(self._handle)
            self._handle = None
        self._playing = False

    def mouseDoubleClickEvent(self, event):
        """Double-clic gauche : bascule plein écran sur l'écran contenant le curseur."""
        if event.button() == Qt.LeftButton:
            win = self.window()
            if win.isFullScreen():
                win.showNormal()
            else:
                screen = QApplication.screenAt(QCursor.pos())
                if screen:
                    win.setGeometry(screen.geometry())
                win.showFullScreen()

    def mouseReleaseEvent(self, event):
        """Clic droit : retour au menu."""
        if event.button() == Qt.RightButton:
            if self._right_click_cb:
                self._right_click_cb()
            return
        super().mouseReleaseEvent(event)

    def contextMenuEvent(self, event):
        """Empêche le menu contextuel par défaut."""
        pass

    def closeEvent(self, event):
        self.stop()
        super().closeEvent(event)
