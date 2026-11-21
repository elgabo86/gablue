// gpad-rec.c — enregistreur / buffer de replay de gablue-pad (v1).
//
// Capture KMS (drmModeGetFB2 -> dmabuf) -> import VAAPI -> conversion GPU ->
// hevc_vaapi/h264_vaapi -> mp4 (enregistrement) et/ou anneau de segments TS
// (replay). Une socket de contrôle remplace pgrep/killall/fichiers d'état.
//
// Contrat complet (config, protocole, pièges) : src/gablue-rec/AGENTS.md.
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <inttypes.h>
#include <pthread.h>
#include <signal.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>

#include <poll.h>
#include <signal.h>
#include <sys/wait.h>

#include <pulse/error.h>
#include <pulse/simple.h>
#include <sys/mman.h>
#include <va/va.h>
#include <libavutil/hwcontext_vaapi.h>
#include <sys/capability.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/un.h>

#include <xf86drm.h>
#include <xf86drmMode.h>
#include <drm_fourcc.h>

#include <libavcodec/avcodec.h>
#include <libavdevice/avdevice.h>
#include <libavfilter/avfilter.h>
#include <libavfilter/buffersink.h>
#include <libavfilter/buffersrc.h>
#include <libavformat/avformat.h>
#include <libavutil/hwcontext.h>
#include <libavutil/audio_fifo.h>
#include <libavutil/hwcontext_drm.h>
#include <libavutil/opt.h>
#include <libswresample/swresample.h>

#define MAX_SEG 512

static void logx(const char *fmt, ...)
{
    va_list ap;
    va_start(ap, fmt);
    fprintf(stderr, "[gpad-rec] ");
    vfprintf(stderr, fmt, ap);
    fputc('\n', stderr);
    va_end(ap);
}

static int64_t maintenant_us(void)
{
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return ts.tv_sec * 1000000LL + ts.tv_nsec / 1000;
}

/* ------------------------------------------------------------------- config */

struct config {
    char codec[8];
    int qualite;
    int plafond;              /* 0 = flips (VFR réel), sinon plafond d'émission */
    int replay_sec;
    int curseur;              /* v2 : mélange de la plane curseur */
    int dix_bits;
    int res_w, res_h;         /* 0 = résolution source */
    int audio;
    int auto_replay;          /* démarrer le replay seul avec un jeu LGP/WGP */
    char dossier[512];
    char socket[512];
};

static void config_defaut(struct config *c)
{
    memset(c, 0, sizeof *c);
    /* défaut = ce qui se partage sans explication : H.264, résolution de
     * l'écran (0x0 = source, le barème de débit suit alors la taille réelle) */
    snprintf(c->codec, sizeof c->codec, "h264");
    c->res_w = 0;
    c->res_h = 0;
    c->qualite = 3;
    c->plafond = 0;           /* 0 = flips (AUTO) : cadence réelle de l'écran */
    c->replay_sec = 30;
    c->audio = 1;
    snprintf(c->dossier, sizeof c->dossier, "%s/Vidéos/Captures", getenv("HOME") ?: ".");
    const char *rt = getenv("XDG_RUNTIME_DIR");
    snprintf(c->socket, sizeof c->socket, "%s/gablue/gpad-rec.sock", rt ? rt : "/run/user/1000");
}

static void config_lit_fichier(struct config *c)
{
    char chemin[700];
    snprintf(chemin, sizeof chemin, "%s/.config/gablue/recorder.conf", getenv("HOME") ?: "");
    FILE *f = fopen(chemin, "r");
    if (!f) return;
    char ligne[600];
    while (fgets(ligne, sizeof ligne, f)) {
        if (ligne[0] == '#' || ligne[0] == '\n') continue;
        char *p = strchr(ligne, '=');
        if (!p) continue;
        *p = 0;
        char *k = ligne, *v = p + 1;
        v[strcspn(v, "\r\n")] = 0;
        if (!strcmp(k, "codec")) snprintf(c->codec, sizeof c->codec, "%s", v);
        else if (!strcmp(k, "qualite")) c->qualite = atoi(v);
        else if (!strcmp(k, "fps")) c->plafond = !strcmp(v, "flips") ? 0 : atoi(v);
        else if (!strcmp(k, "replay_sec")) c->replay_sec = atoi(v);
        else if (!strcmp(k, "curseur")) c->curseur = !strcmp(v, "affiche");
        else if (!strcmp(k, "dix_bits")) c->dix_bits = atoi(v);
        else if (!strcmp(k, "resolution")) {
            if (sscanf(v, "%dx%d", &c->res_w, &c->res_h) != 2) c->res_w = c->res_h = 0;
        }
        else if (!strcmp(k, "audio")) c->audio = strcmp(v, "none") != 0;
        else if (!strcmp(k, "auto_replay")) c->auto_replay = atoi(v);
        else if (!strcmp(k, "dossier")) snprintf(c->dossier, sizeof c->dossier, "%s", v);
    }
    fclose(f);
}

static void config_finalise(struct config *c)
{
    if (getenv("RECORD_DIR")) snprintf(c->dossier, sizeof c->dossier, "%s", getenv("RECORD_DIR"));
    if (getenv("RECORD_RES") && sscanf(getenv("RECORD_RES"), "%dx%d", &c->res_w, &c->res_h) != 2)
        c->res_w = c->res_h = 0;
    if (getenv("RECORD_FPS")) c->plafond = atoi(getenv("RECORD_FPS"));
    if (getenv("RECORD_AUDIO")) c->audio = *getenv("RECORD_AUDIO") != 0;
    if (getenv("REPLAY_SEC")) c->replay_sec = atoi(getenv("REPLAY_SEC"));
    if (c->qualite < 1) c->qualite = 1;
    if (c->qualite > 5) c->qualite = 5;
    if (c->replay_sec < 5) c->replay_sec = 5;
}

/* ------------------------------------------------------------------ capture */

struct capture {
    int card;
    uint32_t plane;
    uint32_t crtc;            /* CRTC de la plane capturée (cherche le curseur) */
    int w, h;
    uint32_t fourcc;
    uint64_t modifier;
    int refresh;              /* Hz du mode de l'écran capturé */
    cap_t caps;
    AVBufferRef *dev, *frames;
};

static drmModeFB2 *fb2_privilegie(struct capture *c, uint32_t id)
{
    cap_value_t cap = CAP_SYS_ADMIN;
    drmModeFB2 *fb;
    if (cap_set_flag(c->caps, CAP_EFFECTIVE, 1, &cap, CAP_SET) || cap_set_proc(c->caps))
        return NULL;
    fb = drmModeGetFB2(c->card, id);
    if (cap_set_flag(c->caps, CAP_EFFECTIVE, 1, &cap, CAP_CLEAR) || cap_set_proc(c->caps)) {
        drmModeFreeFB2(fb);
        return NULL;
    }
    return fb;
}

static enum AVPixelFormat pix_du_fourcc(uint32_t f)
{
    switch (f) {
    case DRM_FORMAT_ARGB2101010: case DRM_FORMAT_XRGB2101010: return AV_PIX_FMT_X2RGB10LE;
    case DRM_FORMAT_ABGR2101010: case DRM_FORMAT_XBGR2101010: return AV_PIX_FMT_X2BGR10LE;
    case DRM_FORMAT_ARGB8888: return AV_PIX_FMT_BGRA;
    case DRM_FORMAT_XRGB8888: return AV_PIX_FMT_BGR0;
    case DRM_FORMAT_ABGR8888: return AV_PIX_FMT_RGBA;
    case DRM_FORMAT_XBGR8888: return AV_PIX_FMT_RGB0;
    case DRM_FORMAT_NV12:     return AV_PIX_FMT_NV12;
    default:                  return AV_PIX_FMT_NONE;
    }
}

static int capture_ouvre(struct capture *c, const char *card)
{
    memset(c, 0, sizeof *c);
    c->card = -1;
    c->caps = cap_get_proc();
    if (!c->caps) return -1;
    cap_value_t cap = CAP_SYS_ADMIN;
    if (cap_set_flag(c->caps, CAP_EFFECTIVE, 1, &cap, CAP_SET) || cap_set_proc(c->caps)) {
        logx("CAP_SYS_ADMIN absent : setcap cap_sys_admin+p sur le binaire");
        return -1;
    }
    if (cap_set_flag(c->caps, CAP_EFFECTIVE, 1, &cap, CAP_CLEAR) || cap_set_proc(c->caps))
        return -1;

    c->card = open(card, O_RDWR | O_CLOEXEC);
    if (c->card < 0) { logx("open %s: %s", card, strerror(errno)); return -1; }
    drmSetClientCap(c->card, DRM_CLIENT_CAP_UNIVERSAL_PLANES, 1);
    /* SANS le cap atomic, libdrm ne rend PAS les propriétés atomiques de plane
     * (FB_ID, CRTC_X/CRTC_Y, CRTC_W/H) : la position du curseur devient
     * illisible alors que la plane est bien active (constaté 21/11/2026). */
    drmSetClientCap(c->card, DRM_CLIENT_CAP_ATOMIC, 1);

    drmModePlaneRes *res = drmModeGetPlaneResources(c->card);
    if (!res) { logx("GetPlaneResources: %s", strerror(errno)); return -1; }
    for (uint32_t i = 0; i < res->count_planes && !c->plane; i++) {
        drmModePlane *pl = drmModeGetPlane(c->card, res->planes[i]);
        if (!pl) continue;
        if (pl->fb_id) c->plane = pl->plane_id;
        drmModeFreePlane(pl);
    }
    drmModeFreePlaneResources(res);
    if (!c->plane) { logx("aucune plane active"); return -1; }

    drmModePlane *pl = drmModeGetPlane(c->card, c->plane);
    drmModeFB2 *fb = fb2_privilegie(c, pl->fb_id);
    if (pl && pl->crtc_id) {
        c->crtc = pl->crtc_id;
        drmModeCrtc *cr = drmModeGetCrtc(c->card, pl->crtc_id);
        if (cr) {
            c->refresh = cr->mode_valid ? cr->mode.vrefresh : 0;
            drmModeFreeCrtc(cr);
        }
    }
    drmModeFreePlane(pl);
    if (!fb) { logx("GetFB2 initial: échec"); return -1; }
    c->w = fb->width;
    c->h = fb->height;
    c->fourcc = fb->pixel_format;
    c->modifier = (fb->flags & DRM_MODE_FB_MODIFIERS) ? fb->modifier : DRM_FORMAT_MOD_INVALID;
    drmModeFreeFB2(fb);

    enum AVPixelFormat pix = pix_du_fourcc(c->fourcc);
    if (pix == AV_PIX_FMT_NONE) { logx("fourcc 0x%08x non géré", c->fourcc); return -1; }

    if (av_hwdevice_ctx_create(&c->dev, AV_HWDEVICE_TYPE_DRM, card, NULL, 0) < 0) {
        logx("hwdevice DRM: échec");
        return -1;
    }
    c->frames = av_hwframe_ctx_alloc(c->dev);
    AVHWFramesContext *fc = (AVHWFramesContext *)c->frames->data;
    fc->format = AV_PIX_FMT_DRM_PRIME;
    fc->sw_format = pix;
    fc->width = c->w;
    fc->height = c->h;
    if (av_hwframe_ctx_init(c->frames) < 0) { logx("frames ctx DRM: échec"); return -1; }
    logx("capture: plane %u %dx%d fourcc 0x%08x, écran %d Hz", c->plane, c->w, c->h, c->fourcc,
         c->refresh);
    return 0;
}

static void libere_desc(void *opaque, uint8_t *data)
{
    AVDRMFrameDescriptor *d = (AVDRMFrameDescriptor *)data;
    (void)opaque;
    for (int i = 0; i < d->nb_objects; i++)
        if (d->objects[i].fd >= 0) close(d->objects[i].fd);
    av_free(d);
}

static uint32_t capture_fb_courant(struct capture *c)
{
    drmModePlane *pl = drmModeGetPlane(c->card, c->plane);
    if (!pl) return 0;
    uint32_t fb = pl->fb_id;
    drmModeFreePlane(pl);
    return fb;
}

/* 0 = ok, >0 = rien à capturer, <0 = le scanout a changé (arrêt propre) */
static int capture_frame(struct capture *c, AVFrame **out)
{
    drmModePlane *pl = drmModeGetPlane(c->card, c->plane);
    if (!pl) return 1;
    uint32_t fb_id = pl->fb_id;
    drmModeFreePlane(pl);
    if (!fb_id) return 1;

    drmModeFB2 *fb = fb2_privilegie(c, fb_id);
    if (!fb) return 1;
    if (!fb->handles[0]) { drmModeFreeFB2(fb); return 1; }
    if ((int)fb->width != c->w || (int)fb->height != c->h || fb->pixel_format != c->fourcc) {
        logx("scanout changé (%ux%u 0x%08x) : arrêt propre", fb->width, fb->height, fb->pixel_format);
        drmModeFreeFB2(fb);
        return -1;
    }

    int fd;
    if (drmPrimeHandleToFD(c->card, fb->handles[0], O_RDONLY, &fd) < 0) {
        drmModeFreeFB2(fb);
        return 1;
    }
    uint64_t mod = (fb->flags & DRM_MODE_FB_MODIFIERS) ? fb->modifier : c->modifier;

    /* ARGB2101010 -> XRGB2101010 : même layout, alpha 0b11 (scanout opaque) */
    uint32_t cour = fb->pixel_format;
    if (cour == DRM_FORMAT_ARGB2101010) cour = DRM_FORMAT_XRGB2101010;
    if (cour == DRM_FORMAT_ABGR2101010) cour = DRM_FORMAT_XBGR2101010;

    AVDRMFrameDescriptor *d = av_mallocz(sizeof(*d));
    d->nb_objects = 1;
    d->objects[0].fd = fd;
    d->objects[0].size = (size_t)fb->height * fb->pitches[0];
    d->objects[0].format_modifier = mod;
    d->nb_layers = 1;
    d->layers[0].format = cour;
    d->layers[0].nb_planes = 1;
    d->layers[0].planes[0].pitch = fb->pitches[0];

    AVFrame *f = av_frame_alloc();
    f->hw_frames_ctx = av_buffer_ref(c->frames);
    f->buf[0] = av_buffer_create((uint8_t *)d, sizeof(*d), libere_desc, NULL, 0);
    f->data[0] = (uint8_t *)d;
    f->format = AV_PIX_FMT_DRM_PRIME;
    f->width = fb->width;
    f->height = fb->height;
    drmModeFreeFB2(fb);
    *out = f;
    return 0;
}

/* -------------------------------------------------------------------- état */

/* Le curseur DRM vit sur une plane SÉPARÉE : il n'est jamais dans le
 * framebuffer capturé. « Afficher » = le relire et le mélanger dans la surface
 * NV12 que l'encodeur va consommer.
 *
 * Pourquoi pas `overlay_vaapi` (le plan initial) : ce filtre n'évalue `x`/`y`
 * qu'à la configuration de son entrée (FFmpeg 9), un curseur qui bouge ne suit
 * donc pas. `vaGetImage`+`vaPutImage` corrompait toute la frame (blit NV12 sur
 * surface tuilée), `vaDeriveImage` + écriture du seul rectangle fonctionne et
 * ne copie rien. */
struct curseur {
    uint32_t plane;          /* plane DRM de type CURSOR (0 = pas encore cherchée) */
    uint32_t fb;             /* dernier framebuffer d'image lu */
    uint8_t *pixels;         /* ARGB mappé (dumb buffer) */
    size_t pixels_len;
    int pitch, w, h;
    int opaque;              /* XRGB8888 : alpha implicite à 255 */
    /* ids des propriétés atomiques de la plane : en mode atomique,
     * `drmModeGetPlane` renvoie crtc_x/y = 0 (la position ne vit que dans les
     * propriétés CRTC_X/CRTC_Y) — mesuré le 21/11/2026, curseur collé en 0,0 */
    uint32_t prop_fb, prop_x, prop_y, prop_w, prop_h;
    int64_t dernier_essai;   /* recherche de la plane (1/s max) */
    int echec_logge;         /* VAAPI ou le mapping a refusé : on n'insiste plus */
};

struct etat {
    struct config cfg;
    struct capture cap;
    struct curseur cur;

    pthread_mutex_t verrou;
    volatile int arret;
    char tampon[256];

    int enc_ouvert;
    AVCodecContext *enc;
    AVFilterGraph *graphe;
    AVFilterContext *src, *sink;

    /* enregistrement */
    int rec_demande, rec_actif;
    char rec_fichier[600], rec_part[640];
    int64_t rec_debut, t0;
    AVFormatContext *mux;
    int v_index, a_index_mux;
    int audio_actif;
    /* Paramètres du flux audio, copiés une fois. L'encodeur AAC lui-même est
     * RECRÉÉ À CHAQUE SESSION : le réutiliser après un drain n'est pas garanti
     * (ffmpeg-devel, « avcodec/nvenc: fix flushing for encoder-reuse » : après
     * `send_frame(NULL)` + flush, les paquets suivants sortent avec un état
     * incohérent — mesuré ici : pts invalides puis EOF définitif, donc plus
     * aucune piste audio dans les clips 2 et suivants). Un encodeur neuf par
     * session coûte quelques ms et supprime toute cette classe de bugs. */
    AVCodecParameters *audio_par;
    AVRational audio_tb;
    double rec_duree;
    /* Origine d'horodatage du mp4, en µs, relative à `t0` : un fichier ouvert
     * en cours de session doit démarrer à 0 (sinon sa piste commence à t0 :
     * mesuré 9,4 s de durée pour 5 s capturées). UNE seule origine pour les
     * deux flux — un recalage par flux décalait l'audio de la latence de son
     * tampon (~0,9 s constaté). */
    int64_t rec_off_us;
    /* Correction d'origine de la piste audio (échelle t0), posée au premier
     * paquet de la session : l'encodeur garde une chronologie cumulative (ses
     * pts ne reculent jamais), le fichier repart de 0. */
    int64_t audio_off;
    int audio_off_pret;      /* origine audio connue pour cette session */

    /* replay */
    int replay_demande, replay_actif;
    AVFormatContext *seg;
    char seg_liste[600];
    double ring_sec;

    int64_t frames;
    int fps_enc;              /* cadence déclarée à l'encodeur */
    int forcer_idr;
    int attendre_idr;
    AVPacket *premiers[8];
    int nb_premiers;
    uint8_t *extradata;
    int extradata_size;
    int srv;             /* socket d'écoute (fermée à l'arrêt) */
    int sauvegarde;      /* sauvegarde en cours : la purge ne supprime rien */
    int redemarrer;      /* reload : fermer moteur + sorties, tout réouvrir */
    int reload_demande;  /* config relue : appliquée dès qu'aucun mp4 n'est ouvert */
    int auto_lance;      /* le replay tourne parce que l'automatisme l'a démarré */
    struct garde *garde; /* maintien de la sortie audio pendant une session */
    char erreur[256];
};

static void erreur(struct etat *e, const char *fmt, ...)
{
    va_list ap;
    va_start(ap, fmt);
    vsnprintf(e->erreur, sizeof e->erreur, fmt, ap);
    va_end(ap);
    logx("%s", e->erreur);
}

static void horodate(char *dst, size_t n)
{
    time_t t = time(NULL);
    struct tm tm;
    localtime_r(&t, &tm);
    strftime(dst, n, "%Y-%m-%d_%H-%M-%S", &tm);
}

/* ------------------------------------------------------------------ curseur */

/* Plane DRM de type CURSOR pour notre CRTC (0 = aucune pour l'instant).
 *
 * Deux cas : la plane est active sur le CRTC (`crtc_id`) ou elle est libre
 * (`crtc_id` 0) — KWin la détache quand le pointeur sort/est masqué, et une
 * plane libre reste utilisable si `possible_crtcs` couvre notre CRTC. Chercher
 * seulement `crtc_id == crtc` ratait la plane au démarrage (constaté
 * 21/11/2026), et le cache la gardait « introuvable » pour toute la session. */
static uint32_t curseur_plane_trouve(int card, uint32_t crtc)
{
    drmModeRes *res = drmModeGetResources(card);
    int idx = -1;
    for (int i = 0; res && i < res->count_crtcs; i++)
        if (res->crtcs[i] == crtc)
            idx = i;
    drmModeFreeResources(res);

    drmModePlaneRes *pr = drmModeGetPlaneResources(card);
    if (!pr)
        return 0;
    uint32_t actif = 0, libre = 0;
    for (uint32_t i = 0; i < pr->count_planes && !actif; i++) {
        drmModePlane *pl = drmModeGetPlane(card, pr->planes[i]);
        if (!pl)
            continue;
        uint32_t id = pl->plane_id, cid = pl->crtc_id, possibles = pl->possible_crtcs;
        drmModeFreePlane(pl);
        drmModeObjectProperties *props =
            drmModeObjectGetProperties(card, id, DRM_MODE_OBJECT_PLANE);
        if (!props)
            continue;
        int type = -1;
        for (uint32_t j = 0; j < props->count_props; j++) {
            drmModePropertyRes *p = drmModeGetProperty(card, props->props[j]);
            if (p) {
                if (!strcmp(p->name, "type"))
                    type = (int)props->prop_values[j];
                drmModeFreeProperty(p);
            }
        }
        drmModeFreeObjectProperties(props);
        if (type != DRM_PLANE_TYPE_CURSOR)
            continue;
        if (cid == crtc)
            actif = id;
        else if (!libre && idx >= 0 && (possibles & (1u << idx)))
            libre = id;
    }
    drmModeFreePlaneResources(pr);
    return actif ? actif : libre;
}

/* Ids des propriétés utiles d'une plane (0 = absente). */
static void curseur_props(int card, struct curseur *cu, uint32_t plane)
{
    cu->prop_fb = cu->prop_x = cu->prop_y = cu->prop_w = cu->prop_h = 0;
    drmModeObjectProperties *pr = drmModeObjectGetProperties(card, plane, DRM_MODE_OBJECT_PLANE);
    if (!pr) {
        logx("curseur: propriétés de la plane %u illisibles", plane);
        return;
    }
    for (uint32_t j = 0; j < pr->count_props; j++) {
        drmModePropertyRes *p = drmModeGetProperty(card, pr->props[j]);
        if (!p)
            continue;
        if (!strcmp(p->name, "FB_ID")) cu->prop_fb = p->prop_id;
        else if (!strcmp(p->name, "CRTC_X")) cu->prop_x = p->prop_id;
        else if (!strcmp(p->name, "CRTC_Y")) cu->prop_y = p->prop_id;
        else if (!strcmp(p->name, "CRTC_W")) cu->prop_w = p->prop_id;
        else if (!strcmp(p->name, "CRTC_H")) cu->prop_h = p->prop_id;
        drmModeFreeProperty(p);
    }
    drmModeFreeObjectProperties(pr);
}

/* Pixels ARGB du curseur : relus seulement quand l'image change (le
 * framebuffer du curseur est réutilisé, seule son image et sa position
 * bougent). KWin le crée en dumb buffer, donc mappable ; un buffer GBM
 * renverrait une erreur, on le dit une fois et on laisse le curseur masqué. */
static void curseur_charge(struct capture *c, struct curseur *cu, uint32_t fb_id)
{
    if (cu->pixels) {
        munmap(cu->pixels, cu->pixels_len);
        cu->pixels = NULL;
        cu->pixels_len = 0;
    }
    cu->fb = fb_id;
    drmModeFB2 *fb = fb2_privilegie(c, fb_id);
    if (!fb)
        return;
    uint32_t format = fb->pixel_format;
    int argb = format == DRM_FORMAT_ARGB8888;
    int xrgb = format == DRM_FORMAT_XRGB8888;
    uint32_t handle = fb->handles[0];
    int w = (int)fb->width, h = (int)fb->height;
    uint64_t modif = (fb->flags & DRM_MODE_FB_MODIFIERS) ? fb->modifier : DRM_FORMAT_MOD_LINEAR;
    drmModeFreeFB2(fb);
    if (!argb && !xrgb) {
        logx("curseur: format 0x%08x non géré (ARGB8888 attendu)", format);
        cu->echec_logge = 1;
        return;
    }
    if (modif != DRM_FORMAT_MOD_LINEAR) {
        /* un buffer tuilé ne se lit pas comme un tableau de pixels */
        logx("curseur: framebuffer tuilé (modifier 0x%llx) : masqué", (unsigned long long)modif);
        cu->echec_logge = 1;
        return;
    }
    /* Dumb buffer : le noyau donne un offset pour mmap. Le pitch n'est pas
     * exposé pour un buffer déjà créé ; pour un curseur il vaut width*4
     * (largeurs 64/128/256, toutes alignées). */
    uint64_t offset = 0;
    if (drmModeMapDumbBuffer(c->card, handle, &offset) != 0) {
        logx("curseur: plane non mappable (pas un dumb buffer) : masqué");
        cu->echec_logge = 1;
        return;
    }
    size_t len = (size_t)w * 4 * (size_t)h;
    void *ptr = mmap(NULL, len, PROT_READ, MAP_SHARED, c->card, (off_t)offset);
    if (ptr == MAP_FAILED) {
        logx("curseur: mmap du dumb buffer: %s", strerror(errno));
        cu->echec_logge = 1;
        return;
    }
    cu->pixels = ptr;
    cu->pitch = w * 4;
    /* ARGB : px[3] = 0 veut dire TRANSPARENT (le curseur dessine sa forme) ;
     * XRGB n'a pas d'alpha du tout, tout est opaque. */
    cu->opaque = xrgb;
    cu->w = w;
    cu->h = h;
    cu->pixels_len = len;
}

/* RGB -> YUV BT.709 limité, l'espace de l'encodage HD. */
static inline void rgb_vers_yuv(int r, int g, int b, int *y, int *u, int *v)
{
    *y = (int)(16.0 + (219.0 / 255.0) * (0.2126 * r + 0.7152 * g + 0.0722 * b) + 0.5);
    *u = (int)(128.0 + (224.0 / 255.0) * (-0.1146 * r - 0.3854 * g + 0.5 * b) + 0.5);
    *v = (int)(128.0 + (224.0 / 255.0) * (0.5 * r - 0.4542 * g - 0.0458 * b) + 0.5);
}

/* Mélange le curseur dans la surface que l'encodeur va lire. Ne fait rien si
 * le réglage est « masqué », si le curseur n'est pas visible, ou si le pilote a
 * déjà refusé une fois (on ne spamme pas le journal à 60 Hz). */
static void curseur_melange(struct etat *e, AVFrame *vf)
{
    struct curseur *cu = &e->cur;
    if (!e->cfg.curseur || cu->echec_logge || !vf->hw_frames_ctx)
        return;
    AVHWFramesContext *fc = (AVHWFramesContext *)vf->hw_frames_ctx->data;
    if (!fc || fc->format != AV_PIX_FMT_VAAPI || !fc->device_ctx)
        return;
    if (fc->sw_format != AV_PIX_FMT_NV12) {
        /* 10 bits (p010) : Layout différent, non traité — le dire une fois. */
        logx("curseur: surface %d non gérée (NV12 seulement)", (int)fc->sw_format);
        cu->echec_logge = 1;
        return;
    }
    if (!cu->plane) {
        /* une fois par seconde au plus : la plane peut apparaître/disparaître
         * (KWin détache le curseur), ce n'est pas une erreur définitive */
        int64_t t = maintenant_us();
        if (cu->dernier_essai && t - cu->dernier_essai < 250000)
            return;
        cu->dernier_essai = t;
        cu->plane = curseur_plane_trouve(e->cap.card, e->cap.crtc);
        if (!cu->plane)
            return;
        curseur_props(e->cap.card, cu, cu->plane);
        logx("curseur: plane %u (crtc %u)", cu->plane, e->cap.crtc);
    }
    if (!cu->prop_fb)
        curseur_props(e->cap.card, cu, cu->plane);
    drmModeObjectProperties *pr =
        drmModeObjectGetProperties(e->cap.card, cu->plane, DRM_MODE_OBJECT_PLANE);
    if (!pr)
        return;
    uint32_t fb = 0;
    int cx = 0, cy = 0, cw = 0, ch = 0;
    for (uint32_t j = 0; j < pr->count_props; j++) {
        uint32_t id = pr->props[j];
        uint64_t v = pr->prop_values[j];
        if (id == cu->prop_fb) fb = (uint32_t)v;
        else if (id == cu->prop_x) cx = (int)(int32_t)v;
        else if (id == cu->prop_y) cy = (int)(int32_t)v;
        else if (id == cu->prop_w) cw = (int)v;
        else if (id == cu->prop_h) ch = (int)v;
    }
    drmModeFreeObjectProperties(pr);
    if (!fb) {
        /* Plane inactive : KWin en change (il y en a quatre de type CURSOR) ou
         * détache le curseur. Garder l'ancienne en cache faisait lire la
         * position d'une frame périmée — curseur absent ou au mauvais endroit.
         * La re-résolution est bornée à 4 par seconde. */
        cu->plane = 0;
        return;
    }
    /* la taille du curseur est celle de son framebuffer : drmModePlane n'expose
     * pas crtc_w/h (API atomique seulement), et KWin ne met pas le curseur à
     * l'échelle par cette plane. */
    if (fb != cu->fb) {
        curseur_charge(&e->cap, cu, fb);
        if (!cu->pixels)
            return;
    }
    if (cw <= 0 || ch <= 0) {
        cw = cu->w;
        ch = cu->h;
    }
    if (cw <= 0 || ch <= 0)
        return;

    /* position dans la surface encodée (la capture peut être réduite) */
    double kx = (double)e->cfg.res_w / e->cap.w;
    double ky = (double)e->cfg.res_h / e->cap.h;
    int dx = (int)(cx * kx), dy = (int)(cy * ky);
    int dw = (int)(cw * kx + 0.5), dh = (int)(ch * ky + 0.5);
    if (dw <= 0 || dh <= 0)
        return;
    int x0 = dx < 0 ? 0 : dx;
    int y0 = dy < 0 ? 0 : dy;
    int x1 = dx + dw > e->cfg.res_w ? e->cfg.res_w : dx + dw;
    int y1 = dy + dh > e->cfg.res_h ? e->cfg.res_h : dy + dh;
    if (x1 <= x0 || y1 <= y0)
        return;
    int iw = x1 - x0, ih = y1 - y0;

    AVVAAPIDeviceContext *dev = fc->device_ctx->hwctx;
    VADisplay dpy = dev->display;
    /* AV_PIX_FMT_VAAPI : l'identifiant de surface vit dans data[3] */
    VASurfaceID sid = (VASurfaceID)(uintptr_t)vf->data[3];
    if (sid == VA_INVALID_SURFACE)
        return;

    /* `vaDeriveImage` donne la surface TELLE QUELLE (pitch et offsets réels) :
     * on n'écrit que le rectangle du curseur. `vaGetImage`+`vaPutImage`
     * corrompait toute la frame sur surface tuilée (mesuré 21/11/2026 : blit
     * NV12 mal recomposé, vert sur toute l'image). */
    VAImage img;
    if (vaDeriveImage(dpy, sid, &img) != VA_STATUS_SUCCESS) {
        logx("curseur: vaDeriveImage refusé : masqué");
        cu->echec_logge = 1;
        return;
    }
    if (img.format.fourcc != VA_FOURCC_NV12) {
        logx("curseur: image dérivée 0x%08x non gérée", img.format.fourcc);
        vaDestroyImage(dpy, img.image_id);
        cu->echec_logge = 1;
        return;
    }
    void *base = NULL;
    if (vaMapBuffer(dpy, img.buf, &base) != VA_STATUS_SUCCESS || !base) {
        vaDestroyImage(dpy, img.image_id);
        logx("curseur: vaMapBuffer refusé : masqué");
        cu->echec_logge = 1;
        return;
    }
    uint8_t *yp = (uint8_t *)base + img.offsets[0];
    uint8_t *uvp = (uint8_t *)base + img.offsets[1];
    int pitch_y = (int)img.pitches[0], pitch_uv = (int)img.pitches[1];
    /* coin haut-gauche du curseur visible, en coordonnées source */
    int sx0 = x0 - dx, sy0 = y0 - dy;
    for (int j = 0; j < ih; j++) {
        int sy = ((sy0 + j) * cu->h) / dh;
        const uint8_t *src = cu->pixels + (size_t)sy * cu->pitch;
        uint8_t *ligne = yp + (size_t)(y0 + j) * pitch_y + (size_t)x0;
        uint8_t *ligne_uv = uvp + (size_t)((y0 + j) / 2) * pitch_uv + (size_t)(x0 / 2) * 2;
        for (int i = 0; i < iw; i++) {
            int sx = ((sx0 + i) * cu->w) / dw;
            const uint8_t *px = src + (size_t)sx * 4;
            int b = px[0], g = px[1], r = px[2], a = cu->opaque ? 255 : px[3];
            if (a == 0)
                continue;
            int ys, us, vs;
            rgb_vers_yuv(r, g, b, &ys, &us, &vs);
            int oy = ligne[i];
            ligne[i] = (uint8_t)((ys * a + oy * (255 - a) + 127) / 255);
            if ((i & 1) == 0) {
                int o = (i / 2) * 2;
                ligne_uv[o] = (uint8_t)((us * a + ligne_uv[o] * (255 - a) + 127) / 255);
                ligne_uv[o + 1] = (uint8_t)((vs * a + ligne_uv[o + 1] * (255 - a) + 127) / 255);
            }
        }
    }
    vaUnmapBuffer(dpy, img.buf);
    vaDestroyImage(dpy, img.image_id);
}

/* ---------------------------------------------------------------- encodeur */

/* Barème : débit de base à 1080p60 en H.264 (le cas de référence), puis
 * mise à l'échelle par pixels et par fps. Le HEVC prend 28 % de moins. */
static void debit_du_niveau(struct etat *e, int64_t *vbr, int64_t *maxr, int *qp)
{
    static const int64_t base[6][2] = {
        {0, 0}, {3000000, 5000000}, {6000000, 10000000},
        {10000000, 16000000}, {16000000, 26000000}, {0, 0},
    };
    int q = e->cfg.qualite;
    *vbr = base[q][0];
    *maxr = base[q][1];
    *qp = 20;
    if (!*vbr) return;                       /* niveau 5 : CQP */
    double px = (double)e->cfg.res_w * e->cfg.res_h / (1920.0 * 1080.0);
    double fr = e->fps_enc / 60.0;
    if (fr < 0.5) fr = 0.5;
    double k = px * fr;
    if (strcmp(e->cfg.codec, "h264")) k *= 0.72;   /* HEVC */
    *vbr = (int64_t)(*vbr * k);
    *maxr = (int64_t)(*maxr * k);
    if (*vbr < 2000000) *vbr = 2000000;
    if (*maxr < *vbr) *maxr = *vbr * 3 / 2;
}

static int graphe_ouvre(struct etat *e)
{
    char chaine[256];
    snprintf(chaine, sizeof chaine, "hwmap=mode=read:derive_device=vaapi,scale_vaapi=w=%d:h=%d:format=%s",
             e->cfg.res_w, e->cfg.res_h,
             (e->cfg.dix_bits && !strcmp(e->cfg.codec, "h265")) ? "p010" : "nv12");

    e->graphe = avfilter_graph_alloc();
    if (!e->graphe) return -1;
    e->src = avfilter_graph_alloc_filter(e->graphe, avfilter_get_by_name("buffer"), "in");
    if (!e->src) return -1;
    AVBufferSrcParameters *par = av_buffersrc_parameters_alloc();
    par->format = AV_PIX_FMT_DRM_PRIME;
    par->width = e->cap.w;
    par->height = e->cap.h;
    par->time_base = (AVRational){ 1, 1000000 };
    par->frame_rate = (AVRational){ 60, 1 };
    par->hw_frames_ctx = av_buffer_ref(e->cap.frames);
    if (av_buffersrc_parameters_set(e->src, par) < 0) { av_freep(&par); return -1; }
    av_freep(&par);
    if (avfilter_init_dict(e->src, NULL) < 0) return -1;

    AVFilterInOut *in = NULL, *out = NULL;
    if (avfilter_graph_parse2(e->graphe, chaine, &in, &out) < 0) return -1;
    if (avfilter_link(e->src, 0, in->filter_ctx, in->pad_idx) < 0) return -1;
    if (avfilter_graph_create_filter(&e->sink, avfilter_get_by_name("buffersink"), "out", NULL, NULL, e->graphe) < 0)
        return -1;
    if (avfilter_link(out->filter_ctx, out->pad_idx, e->sink, 0) < 0) return -1;
    avfilter_inout_free(&in);
    avfilter_inout_free(&out);
    if (avfilter_graph_config(e->graphe, NULL) < 0) {
        erreur(e, "graphe: %s (VPP refuse le format source ?)", chaine);
        return -1;
    }
    return 0;
}

static int encodeur_ouvre(struct etat *e, AVFrame *vf)
{
    const AVCodec *c = avcodec_find_encoder_by_name(
        !strcmp(e->cfg.codec, "h264") ? "h264_vaapi" : "hevc_vaapi");
    if (!c) { erreur(e, "encodeur %s_vaapi absent", e->cfg.codec); return -1; }
    e->enc = avcodec_alloc_context3(c);
    e->enc->pix_fmt = AV_PIX_FMT_VAAPI;
    e->enc->width = vf->width;
    e->enc->height = vf->height;
    e->enc->time_base = (AVRational){ 1, 1000000 };
    /* En VFR le budget par image se calcule sur cette cadence : la déclarer au
     * maximum réel (plafond, sinon le rafraîchissement de l'écran), sinon le
     * débit réel dépasse la cible quand les flips vont plus vite. */
    e->fps_enc = e->cfg.plafond ? e->cfg.plafond : (e->cap.refresh ? e->cap.refresh : 60);
    if (e->fps_enc > 240) e->fps_enc = 240;
    e->enc->framerate = (AVRational){ e->fps_enc, 1 };
    e->enc->gop_size = e->fps_enc;
    e->enc->max_b_frames = 0;
    e->enc->hw_frames_ctx = av_buffer_ref(vf->hw_frames_ctx);
    if (!strcmp(e->cfg.codec, "h264"))
        e->enc->profile = AV_PROFILE_H264_HIGH;
    else
        e->enc->profile = e->cfg.dix_bits ? AV_PROFILE_HEVC_MAIN_10 : AV_PROFILE_HEVC_MAIN;

    int64_t vbr, maxr;
    int qp;
    debit_du_niveau(e, &vbr, &maxr, &qp);
    if (vbr) {
        av_opt_set(e->enc->priv_data, "rc_mode", "VBR", 0);
        e->enc->bit_rate = vbr;
        e->enc->rc_max_rate = maxr;
        e->enc->rc_buffer_size = (int)maxr * 2;
    } else {
        av_opt_set(e->enc->priv_data, "rc_mode", "CQP", 0);
        av_opt_set_int(e->enc->priv_data, "qp", qp, 0);
    }
    /* un I-frame forcé doit être un vrai IDR, sinon le mp4 démarré en cours
     * de route référence des images absentes */
    av_opt_set(e->enc->priv_data, "forced_idr", "1", 0);
    if (avcodec_open2(e->enc, c, NULL) < 0) { erreur(e, "avcodec_open2 %s_vaapi", e->cfg.codec); return -1; }
    e->enc_ouvert = 1;
    logx("encodeur: %s_vaapi %dx%d %s %s%s%" PRId64 " Mbit/s", e->cfg.codec, e->enc->width, e->enc->height,
         vbr ? "VBR" : "CQP", e->cfg.dix_bits ? "10 bits " : "8 bits ", vbr ? "cible " : "",
         vbr ? vbr / 1000000 : 0);
    return 0;
}

/* ------------------------------------------------------- sorties (mp4/ring) */

static void mux_ouvre_mp4(struct etat *e)
{
    char horo[64];
    horodate(horo, sizeof horo);
    if (!e->rec_fichier[0])
        snprintf(e->rec_fichier, sizeof e->rec_fichier, "%s/Video_%s.mp4", e->cfg.dossier, horo);
    snprintf(e->rec_part, sizeof e->rec_part, "%s.part", e->rec_fichier);

    /* L'avcC/hvcC est tiré du premier IDR (l'encodeur VAAPI émet SPS/PPS
     * in-band). Ouvrir avant de l'avoir écrit une entête VIDE : le fichier
     * devient illisible (« missing picture in access unit », avcC de 8 octets).
     * On réessaie simplement à la frame suivante. */
    if (!e->extradata_size) return;
    if (avformat_alloc_output_context2(&e->mux, NULL, "mp4", e->rec_part) < 0) {
        erreur(e, "contexte mp4"); return;
    }
    AVStream *v = avformat_new_stream(e->mux, NULL);
    v->codecpar->codec_type = AVMEDIA_TYPE_VIDEO;
    avcodec_parameters_from_context(v->codecpar, e->enc);
    if (!v->codecpar->extradata && e->extradata) {
        v->codecpar->extradata = av_mallocz(e->extradata_size + AV_INPUT_BUFFER_PADDING_SIZE);
        memcpy(v->codecpar->extradata, e->extradata, e->extradata_size);
        v->codecpar->extradata_size = e->extradata_size;
    }
    v->time_base = e->enc->time_base;
    if (e->enc->codec_id == AV_CODEC_ID_HEVC) v->codecpar->codec_tag = MKTAG('h', 'v', 'c', '1');
    e->v_index = v->index;

    if (e->audio_actif && e->audio_par) {
        AVStream *a = avformat_new_stream(e->mux, NULL);
        avcodec_parameters_copy(a->codecpar, e->audio_par);
        a->time_base = e->audio_tb;
        e->a_index_mux = a->index;
    }
    AVDictionary *mo = NULL;
    /* moov en tête : lecture immédiate dans Discord/WhatsApp, sans attendre
     * la fin du téléchargement */
    av_dict_set(&mo, "movflags", "+faststart", 0);
    if (avio_open(&e->mux->pb, e->rec_part, AVIO_FLAG_WRITE) < 0 ||
        avformat_write_header(e->mux, &mo) < 0) {
        erreur(e, "ouverture %s", e->rec_part);
        avformat_free_context(e->mux);
        e->mux = NULL;
        av_dict_free(&mo);
        return;
    }
    av_dict_free(&mo);
    e->rec_actif = 1;
    e->rec_off_us = e->t0 ? maintenant_us() - e->t0 : 0;
    e->forcer_idr = 1;
    e->attendre_idr = 1;
    e->rec_debut = maintenant_us();
    logx("enregistrement: %s", e->rec_fichier);
}

static void mux_ferme_mp4(struct etat *e, int garder)
{
    if (!e->mux) { e->rec_actif = 0; return; }
    av_write_trailer(e->mux);
    avio_closep(&e->mux->pb);
    avformat_free_context(e->mux);
    e->mux = NULL;
    e->rec_actif = 0;
    e->rec_duree = e->rec_debut ? (maintenant_us() - e->rec_debut) / 1e6 : 0;
    if (garder) {
        if (rename(e->rec_part, e->rec_fichier) == 0)
            logx("enregistré: %s (%.1f s)", e->rec_fichier, e->rec_duree);
        else
            erreur(e, "rename %s", e->rec_fichier);
    } else {
        unlink(e->rec_part);
    }
}

static void ring_ouvre(struct etat *e)
{
    char motif[700];
    snprintf(motif, sizeof motif, "%s/seg%%04d.ts", e->tampon);
    snprintf(e->seg_liste, sizeof e->seg_liste, "%s/liste.csv", e->tampon);
    unlink(e->seg_liste);

    if (avformat_alloc_output_context2(&e->seg, NULL, "segment", motif) < 0) {
        erreur(e, "contexte segment"); return;
    }
    AVDictionary *o = NULL;
    av_dict_set(&o, "segment_time", "1", 0);
    av_dict_set(&o, "segment_format", "mpegts", 0);
    av_dict_set(&o, "segment_list", e->seg_liste, 0);
    av_dict_set(&o, "segment_list_type", "csv", 0);
    av_dict_set(&o, "reset_timestamps", "1", 0);

    AVStream *v = avformat_new_stream(e->seg, NULL);
    v->codecpar->codec_type = AVMEDIA_TYPE_VIDEO;
    avcodec_parameters_from_context(v->codecpar, e->enc);
    if (!v->codecpar->extradata && e->extradata) {
        v->codecpar->extradata = av_mallocz(e->extradata_size + AV_INPUT_BUFFER_PADDING_SIZE);
        memcpy(v->codecpar->extradata, e->extradata, e->extradata_size);
        v->codecpar->extradata_size = e->extradata_size;
    }
    v->time_base = e->enc->time_base;
    if (e->audio_actif && e->audio_par) {
        AVStream *a = avformat_new_stream(e->seg, NULL);
        avcodec_parameters_copy(a->codecpar, e->audio_par);
        a->time_base = e->audio_tb;
    }
    if (avformat_write_header(e->seg, &o) < 0) {
        erreur(e, "segment: entête");
        avformat_free_context(e->seg);
        e->seg = NULL;
        av_dict_free(&o);
        return;
    }
    av_dict_free(&o);
    e->replay_actif = 1;
    e->forcer_idr = 1;
    e->attendre_idr = 1;
    e->ring_sec = 0;
    logx("replay: anneau %d s dans %s", e->cfg.replay_sec, e->tampon);
}

static void ring_ferme(struct etat *e)
{
    if (!e->seg) { e->replay_actif = 0; return; }
    av_write_trailer(e->seg);
    avformat_free_context(e->seg);
    e->seg = NULL;
    e->replay_actif = 0;
}

/* purge : lit la liste CSV du muxeur segment, jette les plus vieux */
/* CSV du muxeur segment : nom,debut,fin (secondes cumulées), chemins relatifs */
static int lit_liste(struct etat *e, char chemins[][700], double *durees, int max)
{
    FILE *f = fopen(e->seg_liste, "r");
    if (!f) return 0;
    int n = 0;
    char ligne[900];
    while (fgets(ligne, sizeof ligne, f) && n < max) {
        char nom[700];
        double t0, t1;
        if (sscanf(ligne, "%699[^,],%lf,%lf", nom, &t0, &t1) != 3) continue;
        const char *b = strrchr(nom, '/');
        b = b ? b + 1 : nom;
        int w = snprintf(chemins[n], 700, "%s/%s", e->tampon, b);
        if (w < 0 || w >= 700) continue;
        durees[n] = t1 - t0;
        n++;
    }
    fclose(f);
    return n;
}

static void ring_purge(struct etat *e)
{
    if (e->sauvegarde) return;
    char chemins[MAX_SEG][700];
    double durees[MAX_SEG];
    int n = lit_liste(e, chemins, durees, MAX_SEG);
    double total = 0;
    for (int i = 0; i < n; i++) {
        struct stat st;
        if (stat(chemins[i], &st) == 0) total += durees[i];
    }
    int i = 0;
    while (i < n && total > e->cfg.replay_sec) {
        struct stat st;
        if (stat(chemins[i], &st) == 0) {
            unlink(chemins[i]);
            total -= durees[i];
        }
        i++;
    }
    e->ring_sec = total;
}

/* sauvegarde : concat des segments restants -> mp4 (copie de flux) */
static int ring_sauve(struct etat *e, char *final, size_t n)
{
    char chemins[MAX_SEG][700];
    double durees[MAX_SEG];
    int nb = lit_liste(e, chemins, durees, MAX_SEG);
    if (nb < 1) { erreur(e, "anneau vide"); return -1; }

    /* garder les ~replay_sec dernières secondes */
    double total = 0;
    int debut = nb;
    while (debut > 0 && total < e->cfg.replay_sec) {
        debut--;
        total += durees[debut];
    }

    char liste[700];
    snprintf(liste, sizeof liste, "%s/sauve.txt", e->tampon);
    FILE *l = fopen(liste, "w");
    if (!l) return -1;
    int nb_ecrits = 0;
    for (int i = debut; i < nb; i++) {
        struct stat st;
        if (stat(chemins[i], &st) == 0) {
            fprintf(l, "file '%s'\n", chemins[i]);
            nb_ecrits++;
        }
    }
    fclose(l);
    if (!nb_ecrits) { erreur(e, "anneau: segments disparus"); return -1; }

    char horo[64];
    horodate(horo, sizeof horo);
    snprintf(final, n, "%s/Replay_%s.mp4", e->cfg.dossier, horo);
    char part[700];
    snprintf(part, sizeof part, "%s.part", final);

    const AVInputFormat *concat = av_find_input_format("concat");
    AVFormatContext *in = NULL;
    AVDictionary *opts = NULL;
    av_dict_set(&opts, "safe", "0", 0);
    if (avformat_open_input(&in, liste, concat, &opts) < 0) {
        av_dict_free(&opts);
        erreur(e, "concat: ouverture de %s", liste);
        return -1;
    }
    av_dict_free(&opts);
    if (avformat_find_stream_info(in, NULL) < 0) { avformat_close_input(&in); return -1; }

    AVFormatContext *out = NULL;
    avformat_alloc_output_context2(&out, NULL, "mp4", part);
    for (unsigned i = 0; i < in->nb_streams; i++) {
        AVStream *s = avformat_new_stream(out, NULL);
        if (avcodec_parameters_copy(s->codecpar, in->streams[i]->codecpar) < 0) return -1;
        s->codecpar->codec_tag = 0;
        s->time_base = in->streams[i]->time_base;
        if (s->codecpar->codec_id == AV_CODEC_ID_HEVC) s->codecpar->codec_tag = MKTAG('h','v','c','1');
    }
    if (avio_open(&out->pb, part, AVIO_FLAG_WRITE) < 0 || avformat_write_header(out, NULL) < 0) {
        erreur(e, "sauvegarde: %s", part);
        avformat_close_input(&in);
        return -1;
    }
    AVPacket *pkt = av_packet_alloc();
    /* le démultiplexeur concat recolle les segments par leurs durées : à la
     * jonction, une frame peut retomber sur un horodatage déjà écrit. On la
     * jette (une frame par jonction) plutôt que d'écrire un mp4 non monotone. */
    int64_t dernier[8];
    for (int i = 0; i < 8; i++) dernier[i] = INT64_MIN;
    int jetees = 0;
    while (av_read_frame(in, pkt) >= 0) {
        AVStream *si = in->streams[pkt->stream_index], *so = out->streams[pkt->stream_index];
        av_packet_rescale_ts(pkt, si->time_base, so->time_base);
        if (pkt->dts != AV_NOPTS_VALUE && pkt->dts <= dernier[pkt->stream_index]) {
            jetees++;
            av_packet_unref(pkt);
            continue;
        }
        if (pkt->dts != AV_NOPTS_VALUE) dernier[pkt->stream_index] = pkt->dts;
        av_interleaved_write_frame(out, pkt);
        av_packet_unref(pkt);
    }
    if (jetees) logx("sauvegarde: %d paquet(s) en collision d'horodatage jetés", jetees);
    av_packet_free(&pkt);
    av_write_trailer(out);
    avio_closep(&out->pb);
    avformat_free_context(out);
    avformat_close_input(&in);
    if (rename(part, final) == 0) {
        logx("replay sauvegardé: %s (%.1f s)", final, total);
        return (int)(total * 1000);
    }
    erreur(e, "rename %s", final);
    return -1;
}

/* ---------------------------------------------------------------- audio */

/* `off_us` : origine du fichier en µs (NULL = horodatage intact, cas des
 * segments de l'anneau). Les paquets ANTÉRIEURS à l'ouverture sont jetés —
 * l'audio est lu avec ~1 s de tampon, les écrire à 0 collerait du son d'avant
 * le début du clip sur la première image. */
static void ecrit_ref(AVFormatContext *mux, int idx, AVPacket *src, AVRational tb,
                      int64_t off_us, int recaler)
{
    AVPacket *p = av_packet_alloc();
    if (!p || av_packet_ref(p, src) < 0) { av_packet_free(&p); return; }
    if (recaler) {
        static const AVRational us = { 1, 1000000 };
        int64_t pt = p->pts == AV_NOPTS_VALUE ? p->dts : p->pts;
        if (pt != AV_NOPTS_VALUE) {
            if (av_rescale_q(pt, tb, us) < off_us) { av_packet_free(&p); return; }
            int64_t dec = av_rescale_q(off_us, us, tb);
            p->pts -= dec;
            if (p->dts != AV_NOPTS_VALUE) p->dts -= dec;
        }
    }
    av_packet_rescale_ts(p, tb, mux->streams[idx]->time_base);
    p->stream_index = idx;
    int rr = av_interleaved_write_frame(mux, p);   /* prend possession de p */
    if (rr < 0) {
        char e[128] = {0};
        av_strerror(rr, e, sizeof e);
        logx("écriture: échec sur le flux %d : %s", idx, e);
    }
    av_packet_free(&p);
}

/* Le muxeur MP4 exige l'extradata (hvcC/avcC) dans l'entête, alors que
 * l'encodeur VAAPI émet les VPS/SPS/PPS in-band. On les recopie depuis le
 * premier IDR — sinon tout mp4 ouvert en cours de route est illisible. */
static void memoire_extradata(struct etat *e, AVPacket *pkt)
{
    if (e->extradata || !(pkt->flags & AV_PKT_FLAG_KEY))
        return;
    int hevc = e->enc && e->enc->codec_id == AV_CODEC_ID_HEVC;
    uint8_t buf[4096];
    int n = 0;
    const uint8_t *p = pkt->data, *fin = pkt->data + pkt->size;

    while (p + 4 < fin) {
        if (!(p[0] == 0 && p[1] == 0 && p[2] == 1)) { p++; continue; }
        const uint8_t *nal = p + 3;
        int type = hevc ? (nal[0] >> 1) & 0x3f : nal[0] & 0x1f;
        const uint8_t *q = nal;
        while (q + 3 < fin && !(q[0] == 0 && q[1] == 0 && q[2] == 1)) q++;
        int len = (int)(q - nal);
        int garder = hevc ? (type == 32 || type == 33 || type == 34) : (type == 7 || type == 8);
        if (garder && len > 0 && n + 3 + len < (int)sizeof buf) {
            buf[n++] = 0; buf[n++] = 0; buf[n++] = 1;
            memcpy(buf + n, nal, len);
            n += len;
        }
        p = q;
    }
    if (!n) return;
    e->extradata = av_mallocz(n + AV_INPUT_BUFFER_PADDING_SIZE);
    if (!e->extradata) return;
    memcpy(e->extradata, buf, n);
    e->extradata_size = n;
    logx("extradata extraite du premier IDR: %d octets", n);
}

static void sortie_paquet(struct etat *e, AVPacket *pkt, int video, AVRational tb)
{
    if (!video && !e->audio_actif)
        return;
    if (video) {
        memoire_extradata(e, pkt);
        /* l'encodeur a quelques frames de retard : ne jamais commencer un
         * fichier sur une image qui référence des frames hors fichier */
        if (e->attendre_idr) {
            if (!(pkt->flags & AV_PKT_FLAG_KEY)) return;
            e->attendre_idr = 0;
            /* Le fichier DÉMARRE à cette frame : avec un PTS de grille, une
             * origine prise à l'horloge murale tombait après elle et la
             * première image clé était jetée (fichier sans avcC). */
            if (e->rec_actif && e->mux && pkt->pts != AV_NOPTS_VALUE)
                e->rec_off_us = av_rescale_q(pkt->pts, tb, (AVRational){ 1, 1000000 });
        }
    }
    pthread_mutex_lock(&e->verrou);
    if (e->rec_actif && e->mux) {
        int64_t off = video ? e->rec_off_us : e->rec_off_us + e->audio_off;
        int pret = video || e->audio_off_pret;
        ecrit_ref(e->mux, video ? e->v_index : e->a_index_mux, pkt, tb, off, pret);
    }
    if (e->replay_actif && e->seg)
        ecrit_ref(e->seg, video ? 0 : 1, pkt, tb, 0, 0);
    pthread_mutex_unlock(&e->verrou);
}

/* Maintien de la sortie audio : un flux de LECTURE muet sur le sink par
 * défaut. Sans lui, le sink s'endort dès qu'aucun son ne joue ; le monitor
 * d'enregistrement ne délivre alors PLUS RIEN (mesuré : la piste audio
 * s'arrêtait au milieu du clip, puis plus aucun paquet) — un enregistreur ne
 * doit pas dépendre du hasard d'un son en cours. ~192 Ko/s de zéros, aucun CPU,
 * et il n'est ouvert que pendant une session. */
struct garde {
    pa_simple *s;
    volatile int arret;
    pthread_t fil;
    int actif;
};

static void *thread_garde(void *arg)
{
    struct garde *g = arg;
    static const uint8_t silence[4800 * 4] = { 0 };   /* 100 ms s16 stéréo */
    pa_sample_spec ss = { PA_SAMPLE_S16LE, 48000, 2 };
    int err = 0;
    g->s = pa_simple_new(NULL, "gpad-rec", PA_STREAM_PLAYBACK, NULL, "monitor",
                         &ss, NULL, NULL, &err);
    if (!g->s) { logx("audio: maintien du sink indisponible (%s)", pa_strerror(err)); return NULL; }
    while (!g->arret) pa_simple_write(g->s, silence, sizeof silence, &err);
    pa_simple_free(g->s);
    g->s = NULL;
    return NULL;
}

static void garde_ouvre(struct garde *g)
{
    if (g->actif) return;
    g->arret = 0;
    if (pthread_create(&g->fil, NULL, thread_garde, g) == 0) g->actif = 1;
}

static void garde_ferme(struct garde *g)
{
    if (!g->actif) return;
    g->arret = 1;
    pthread_join(g->fil, NULL);
    g->actif = 0;
}

/* ---------------------------------------------------------- replay auto */

/* Jeu LGP/WGP en cours, d'après la publication des lanceurs
 * (`$XDG_RUNTIME_DIR/gablue/running.txt`, `paquet=/chemin/x.lgp|.wgp`).
 * Les lanceurs sont la seule source fiable : le `.lgp` n'apparaît ni dans la
 * classe de fenêtre ni dans le nom du processus. */
static int jeu_paquet(char *out, size_t n)
{
    const char *rt = getenv("XDG_RUNTIME_DIR");
    char f[512];
    if (rt && *rt) snprintf(f, sizeof f, "%s/gablue/running.txt", rt);
    else snprintf(f, sizeof f, "/tmp/gablue-%d/running.txt", (int)getuid());
    FILE *fp = fopen(f, "r");
    if (!fp) return 0;
    char ligne[1200];
    int trouve = 0;
    while (fgets(ligne, sizeof ligne, fp)) {
        char *v = strstr(ligne, "paquet=");
        if (!v) continue;
        v += 7;
        v[strcspn(v, "\r\n")] = 0;
        size_t l = strlen(v);
        if (l > 4 && (!strcasecmp(v + l - 4, ".lgp") || !strcasecmp(v + l - 4, ".wgp"))) {
            snprintf(out, n, "%s", v);
            trouve = 1;
        }
        break;
    }
    fclose(fp);
    return trouve;
}

/* Date de modification de recorder.conf (nanosecondes, 0 si absent) : le
 * fichier a plusieurs écrivains (overlay, Gablue Manager, un éditeur). Le
 * démon le surveille lui-même — aucun écrivain n'a besoin de connaître la
 * socket, et aucun oubli de `reload` ne peut laisser un réglage inappliqué. */
static int64_t mtime_conf(void)
{
    char chemin[700];
    snprintf(chemin, sizeof chemin, "%s/.config/gablue/recorder.conf", getenv("HOME") ?: "");
    struct stat st;
    if (stat(chemin, &st) < 0) return 0;
    return (int64_t)st.st_mtim.tv_sec * 1000000000LL + st.st_mtim.tv_nsec;
}

/* N'agit que sur les TRANSITIONS (jeu qui apparaît, jeu qui disparaît) : un
 * replay arrêté à la main ne redémarre pas tout seul pendant la partie.
 *
 * C'est aussi lui qui maintient la sortie audio réveillée pendant une session :
 * le faire fermer par le thread audio serait un interblocage (ce thread peut
 * être en train d'attendre un son dans `av_read_frame`, et c'est justement le
 * maintien qui le débloque). Il est refermé après quelques secondes de repos. */
static void *thread_auto(void *arg)
{
    struct etat *e = arg;
    char jeu[1024] = {0}, suivi[1024] = {0};
    int64_t conf_vue = mtime_conf();
    int repos = 0;
    while (!e->arret) {
        struct timespec ts = { 0, 500 * 1000000L };
        nanosleep(&ts, NULL);
        int besoin = e->rec_demande || e->replay_demande;
        if (e->garde) {
            if (besoin) { garde_ouvre(e->garde); repos = 0; }
            else if (e->garde->actif && ++repos > 6) { garde_ferme(e->garde); repos = 0; }
        }
        int64_t m = mtime_conf();
        if (m != conf_vue) {
            conf_vue = m;
            pthread_mutex_lock(&e->verrou);
            e->reload_demande = 1;
            pthread_mutex_unlock(&e->verrou);
        }
        int actif = e->cfg.auto_replay && jeu_paquet(jeu, sizeof jeu);
        pthread_mutex_lock(&e->verrou);
        if (actif && strcmp(jeu, suivi)) {
            snprintf(suivi, sizeof suivi, "%s", jeu);
            if (!e->replay_demande) {
                e->replay_demande = 1;
                e->auto_lance = 1;
                logx("replay auto: %s", jeu);
            }
        } else if (!actif && suivi[0]) {
            if (e->auto_lance) {
                e->replay_demande = 0;
                e->auto_lance = 0;
                logx("replay auto: arrêt (fin du jeu)");
            }
            suivi[0] = 0;
        }
        pthread_mutex_unlock(&e->verrou);
    }
    return NULL;
}

/* ---------------------------------------------------------------- threads */

static void *thread_video(void *arg)
{
    struct etat *e = arg;
    uint32_t dernier_fb = 0;
    int64_t dernier_emit = 0;
    int64_t grille = 0;          /* échéance de cadence (grille fixe) */
    int64_t nb_pts = 0;          /* index de la frame dans la grille (PTS) */
    AVFrame *vf = av_frame_alloc();

    while (!e->arret) {
        int64_t t = maintenant_us();

        /* arrêts et redémarrages demandés par la socket */
        pthread_mutex_lock(&e->verrou);
        if (!e->rec_demande && e->rec_actif) mux_ferme_mp4(e, 1);
        if (!e->replay_demande && e->replay_actif) ring_ferme(e);
        if (e->reload_demande && !e->rec_actif) {
            config_lit_fichier(&e->cfg);
            config_finalise(&e->cfg);
            e->reload_demande = 0;
            e->redemarrer = 1;
            logx("config relue (reload)");
        }
        if (e->redemarrer) {
            if (e->rec_actif) mux_ferme_mp4(e, 1);
            if (e->replay_actif) ring_ferme(e);
            if (e->graphe) { avfilter_graph_free(&e->graphe); e->src = e->sink = NULL; }
            if (e->enc) avcodec_free_context(&e->enc);
            /* l'extradata (avcC/hvcC) appartient au codec précédent : la garder
             * écrivait un avcC dans un fichier HEVC (illisible) */
            av_freep(&e->extradata);
            e->extradata_size = 0;
            e->enc_ouvert = 0;
            e->t0 = 0;
            e->redemarrer = 0;
        }
        pthread_mutex_unlock(&e->verrou);

        int besoin = e->rec_demande || e->replay_demande;
        if (!besoin) {
            /* Au repos on libère le moteur ET on remet la base de pts à zéro :
             * réutiliser un encodeur ouvert pendant une session précédente
             * donnait un mp4 dont la piste démarrait à t0 (mesuré 54 s de
             * durée pour 3 s capturées). Le démon étant un service de session,
             * libérer le GPU au repos est aussi ce qu'on veut. */
            pthread_mutex_lock(&e->verrou);
            if (e->rec_actif) mux_ferme_mp4(e, 1);
            if (e->replay_actif) ring_ferme(e);
            if (e->graphe) { avfilter_graph_free(&e->graphe); e->src = e->sink = NULL; }
            if (e->enc) avcodec_free_context(&e->enc);
            e->enc_ouvert = 0;
            e->t0 = 0;
            e->frames = 0;
            pthread_mutex_unlock(&e->verrou);
            dernier_fb = 0;
            dernier_emit = 0;
            grille = 0;
            /* 30 ms : un démon de session ne doit pas réveiller le CPU 500 fois
             * par seconde pour ne rien faire */
            usleep(30000);
            continue;
        }

        uint32_t fb = capture_fb_courant(&e->cap);
        if (!fb) { usleep(500); continue; }

        /* Deux régimes de cadence.
         *
         * Plafonné (défaut) : cadence FIXE, à l'échéance absolue. Surtout pas
         * de test « FB_ID a changé » : le compositeur recycle 2-3 buffers, donc
         * l'id revient sur une frame pourtant neuve → le démon croyait que rien
         * n'avait bougé et attendait le flip suivant (1,5 ms perdus par frame,
         * 55 fps au lieu de 60 ; mesuré 21/11/2026 en 1440p comme en 720p).
         * Une frame identique en trop coûte un P-frame quasi vide, alors qu'une
         * frame sautée se voit.
         *
         * `fps=flips` (VFR) : on émet à chaque changement de scanout, au mieux
         * — l'id recyclé y fait perdre quelques frames, d'où le défaut CFR. */
        if (e->cfg.plafond) {
            int64_t echeance = dernier_emit + 1000000LL / e->cfg.plafond;
            if (grille) echeance = grille;
            if (t < echeance) {
                /* échéance ABSOLUE : une boucle `usleep(300)` cumulait son
                 * retard de réveil à chaque frame */
                struct timespec ts = { (time_t)(echeance / 1000000),
                                       (long)(echeance % 1000000) * 1000 };
                clock_nanosleep(CLOCK_MONOTONIC, TIMER_ABSTIME, &ts, NULL);
                continue;
            }
        } else if (fb == dernier_fb) {
            usleep(300);
            continue;
        }

        /* Instant RÉEL de la capture. Pas `t` (haut de boucle) : entre les deux
         * il y a l'attente du framebuffer et le sommeil de plafond, jusqu'à
         * 15 ms — le PTS traînait donc ce retard et le fichier sortait à
         * 51-55 fps alors que les captures étaient à 60 (mesuré 21/11/2026). */
        int64_t t1 = maintenant_us();
        AVFrame *f = NULL;
        int r = capture_frame(&e->cap, &f);
        if (r < 0) { e->arret = 1; break; }
        if (r > 0) { usleep(500); continue; }

        if (!e->graphe && graphe_ouvre(e) < 0) { e->arret = 1; break; }
        if (!e->t0) { e->t0 = t1; nb_pts = 0; }
        /* PTS de la GRILLE, pas de l'horloge : (n x 1 s / fps) donne 16666 puis
         * 16667 us et une cadence exacte. Avec l'horloge réelle, un réveil
         * tardif de 5 ms faisait tomber un 30 s à 59,97 fps. Le décalage de
         * contenu correspondant reste sous la milliseconde. */
        int64_t pts = e->cfg.plafond
            ? (nb_pts * 1000000LL) / e->cfg.plafond
            : t1 - e->t0;
        f->pts = pts;
        if (av_buffersrc_add_frame_flags(e->src, f, 0) < 0) { av_frame_free(&f); continue; }
        av_frame_free(&f);

        r = av_buffersink_get_frame(e->sink, vf);
        if (r < 0) { usleep(300); continue; }
        curseur_melange(e, vf);

        if (!e->enc_ouvert) {
            if (encodeur_ouvre(e, vf) < 0) { e->arret = 1; break; }
            /* amorce : encoder la 1re frame donne l'extradata et un IDR.
             * Les sorties ne peuvent s'ouvrir qu'APRÈS, sinon l'entête mp4
             * part sans hvcC (fichier illisible). */
            AVFrame *a = av_frame_clone(vf);
            a->pts = pts;   /* même index de grille que la frame courante */
            a->pict_type = AV_PICTURE_TYPE_I;
            e->forcer_idr = 0;
            if (avcodec_send_frame(e->enc, a) == 0) {
                AVPacket *q = av_packet_alloc();
                while (e->nb_premiers < 8 && avcodec_receive_packet(e->enc, q) == 0) {
                    if (!(e->premiers[e->nb_premiers] = av_packet_clone(q))) break;
                    e->nb_premiers++;
                    av_packet_unref(q);
                }
                av_packet_free(&q);
            }
            av_frame_free(&a);
            if (e->nb_premiers) memoire_extradata(e, e->premiers[0]);

            pthread_mutex_lock(&e->verrou);
            if (e->rec_demande && !e->rec_actif) mux_ouvre_mp4(e);
            if (e->replay_demande && !e->replay_actif) ring_ouvre(e);
            pthread_mutex_unlock(&e->verrou);

            for (int k = 0; k < e->nb_premiers; k++) {
                sortie_paquet(e, e->premiers[k], 1, e->enc->time_base);
                e->frames++;
                av_packet_free(&e->premiers[k]);
            }
            e->nb_premiers = 0;
            av_frame_unref(vf);
            dernier_fb = fb;
            dernier_emit = t;
            nb_pts++;
            continue;
        }
        /* démarrage à chaud : sortie demandée après l'ouverture de l'encodeur */
        pthread_mutex_lock(&e->verrou);
        if (e->rec_demande && !e->rec_actif) mux_ouvre_mp4(e);
        if (e->replay_demande && !e->replay_actif) ring_ouvre(e);
        pthread_mutex_unlock(&e->verrou);

        vf->pts = pts;
        if (e->forcer_idr) { vf->pict_type = AV_PICTURE_TYPE_I; e->forcer_idr = 0; }
        if (avcodec_send_frame(e->enc, vf) < 0) { av_frame_unref(vf); continue; }
        av_frame_unref(vf);
        nb_pts++;

        AVPacket *pkt = av_packet_alloc();
        while (avcodec_receive_packet(e->enc, pkt) == 0) {
            sortie_paquet(e, pkt, 1, e->enc->time_base);
            e->frames++;
            av_packet_unref(pkt);
        }
        av_packet_free(&pkt);
        dernier_fb = fb;
        /* Pas `t` (haut de boucle) : le traitement entre les deux comptait comme
         * du temps de frame et décalait l'échéance suivante.
         * `grille` = cadence sur une grille fixe, pas « dernière émission +
         * période » : le coût d'une frame (capture, filtre, encodage) se
         * rajoutait à chaque période (60 rendait 59,4 fps). Si on a pris du
         * retard au-delà d'une période entière, on recale au lieu de rattraper
         * en rafale. */
        int64_t em = maintenant_us();
        if (e->cfg.plafond) {
            int64_t periode = 1000000LL / e->cfg.plafond;
            grille = grille ? grille + periode : em + periode;
            if (grille < em) grille = em + periode;
        }
        dernier_emit = em;

        /* purge de l'anneau une fois par seconde environ */
        static int64_t dernier_purge;
        if (e->replay_actif && t - dernier_purge > 1000000) {
            pthread_mutex_lock(&e->verrou);
            ring_purge(e);
            pthread_mutex_unlock(&e->verrou);
            dernier_purge = t;
        }
    }

    av_frame_free(&vf);
    pthread_mutex_lock(&e->verrou);
    if (e->rec_actif) mux_ferme_mp4(e, 1);
    if (e->replay_actif) ring_ferme(e);
    pthread_mutex_unlock(&e->verrou);
    return NULL;
}

/* ------------------------------------------------------------------ audio */

struct audio {
    struct garde garde;   /* maintien de la sortie audio réveillée */
    pid_t pid;            /* `parec` de la session courante (0 = aucun) */
    int fd;               /* tube de sa sortie PCM (-1 = fermé) */
    AVCodecContext *enc;  /* encodeur de la session courante (NULL au repos) */
    int fs;               /* frame_size de l'encodeur */
    SwrContext *swr;
    AVAudioFifo *fifo;
    int64_t emis;         /* échantillons déjà encodés dans la session */
    struct etat *e;
    volatile int arret;
};

/* Capture PCM par `parec`, UN PROCESSUS PAR SESSION.
 *
 * Pourquoi pas le décodeur `pulse` d'FFmpeg : sa lecture attend un signal de
 * PulseAudio sans aucun crochet d'interruption (libavdevice/pulse_audio_dec.c,
 * `pa_threaded_mainloop_wait`). Une fois la sortie audio endormie, le thread
 * restait bloqué À VIE — mesuré : seule la première session avait une piste
 * audio, les suivantes rien du tout, même avec du son qui jouait. Un tube lu
 * avec `poll()` ne peut pas bloquer : la boucle garde la main sur l'état.
 */
static int audio_capture_ouvre(struct audio *a)
{
    int p[2];
    if (pipe2(p, O_CLOEXEC) < 0) return -1;
    pid_t pid = fork();
    if (pid == 0) {
        dup2(p[1], STDOUT_FILENO);
        close(p[0]);
        close(p[1]);
        execlp("parec", "parec", "--raw", "--format=s16le", "--rate=48000",
               "--channels=2", "--device=@DEFAULT_MONITOR@", "--latency-msec=60",
               (char *)NULL);
        _exit(127);
    }
    if (pid < 0) { close(p[0]); close(p[1]); return -1; }
    close(p[1]);
    a->pid = pid;
    a->fd = p[0];
    return 0;
}

static void audio_capture_ferme(struct audio *a)
{
    if (a->fd >= 0) { close(a->fd); a->fd = -1; }
    if (a->pid <= 0) return;
    kill(a->pid, SIGTERM);
    for (int i = 0; i < 40; i++) {           /* 200 ms, puis on insiste */
        if (waitpid(a->pid, NULL, WNOHANG) == a->pid) { a->pid = 0; return; }
        usleep(5000);
    }
    kill(a->pid, SIGKILL);
    waitpid(a->pid, NULL, 0);
    a->pid = 0;
}

/* Ouvre un encodeur AAC neuf pour une session. Ses paramètres (extradata
 * compris) sont identiques d'une session à l'autre : c'est le gabarit copié
 * dans `e->audio_par` qui sert aux flux du mux. */
static int audio_encodeur_neuf(struct audio *a)
{
    const AVCodec *c = avcodec_find_encoder(AV_CODEC_ID_AAC);
    if (!c) { logx("encodeur aac absent"); return -1; }
    a->enc = avcodec_alloc_context3(c);
    a->enc->sample_fmt = AV_SAMPLE_FMT_FLTP;
    a->enc->sample_rate = 48000;
    av_channel_layout_default(&a->enc->ch_layout, 2);
    a->enc->bit_rate = 192000;
    a->enc->time_base = (AVRational){ 1, 1000000 };
    if (avcodec_open2(a->enc, c, NULL) < 0) {
        logx("aac: open");
        avcodec_free_context(&a->enc);
        return -1;
    }
    a->fs = a->enc->frame_size > 0 ? a->enc->frame_size : 1024;
    a->emis = 0;
    if (a->fifo) av_audio_fifo_reset(a->fifo);
    return 0;
}

static int audio_ouvre(struct audio *a, struct etat *e)
{
    memset(a, 0, sizeof *a);
    a->e = e;
    a->fd = -1;

    if (audio_encodeur_neuf(a) < 0) return -1;
    /* gabarit des flux du mux, puis l'encodeur de ce gabarit est jeté : la
     * première session en ouvrira un à son tour */
    e->audio_par = avcodec_parameters_alloc();
    if (!e->audio_par || avcodec_parameters_from_context(e->audio_par, a->enc) < 0) {
        logx("aac: paramètres");
        return -1;
    }
    e->audio_tb = a->enc->time_base;

    /* swr est bâti AVANT de jeter le gabarit (il a besoin de sa disposition
     * de canaux) ; le FIFO aussi, il doit survivre aux sessions */
    AVChannelLayout st;
    av_channel_layout_default(&st, 2);
    if (swr_alloc_set_opts2(&a->swr, &a->enc->ch_layout, AV_SAMPLE_FMT_FLTP, 48000,
                            &st, AV_SAMPLE_FMT_S16, 48000, 0, NULL) < 0 || swr_init(a->swr) < 0) {
        logx("swr: init");
        return -1;
    }
    a->fifo = av_audio_fifo_alloc(AV_SAMPLE_FMT_FLTP, 2, 48000);
    avcodec_free_context(&a->enc);
    logx("audio: monitor du sink, aac 192k");
    return 0;
}

static void *thread_audio(void *arg)
{
    struct audio *a = arg;
    struct etat *e = a->e;
    AVPacket *pkt = av_packet_alloc();
    AVFrame *conv = av_frame_alloc();
    uint8_t *pcm = av_malloc(48000);   /* 250 ms s16 stéréo */

    conv->format = AV_SAMPLE_FMT_FLTP;
    conv->sample_rate = 48000;
    av_channel_layout_default(&conv->ch_layout, 2);
    conv->nb_samples = 8192;
    av_frame_get_buffer(conv, 0);

    int64_t t0_vu = 0;
    while (!e->arret && !a->arret) {
        int besoin = e->rec_demande || e->replay_demande;
        if (!besoin) {
            /* repos : capture et encodeur de la session sont jetés ensemble.
             * Rien à drainer : les trames en attente de l'encodeur
             * appartiennent au fichier qu'on vient de fermer. */
            if (a->fd >= 0) audio_capture_ferme(a);
            if (a->enc) avcodec_free_context(&a->enc);
            usleep(20000);
            continue;
        }
        if (!e->t0) { usleep(2000); continue; }

        if (e->t0 != t0_vu) {
            t0_vu = e->t0;
            if (a->fifo) av_audio_fifo_reset(a->fifo);
            /* chaque session a sa propre origine : la piste du clip repart de
             * 0 et `audio_off` l'aligne sur l'image au premier échantillon */
            e->audio_off = 0;
            e->audio_off_pret = 0;
        }
        if (a->fd < 0 && audio_capture_ouvre(a) < 0) { usleep(200000); continue; }
        if (!a->enc && audio_encodeur_neuf(a) < 0) { usleep(200000); continue; }

        /* lecture bornée : jamais plus de 100 ms d'attente, donc l'état de la
         * session est relu même quand la sortie audio ne produit rien */
        struct pollfd pf = { .fd = a->fd, .events = POLLIN };
        int pret = poll(&pf, 1, 100);
        if (pret <= 0) continue;
        ssize_t lus = read(a->fd, pcm, 48000);
        if (lus <= 0) { audio_capture_ferme(a); continue; }

        /* horodatage : `contenu` = temps écoulé depuis t0, à la lecture */
        int64_t contenu = maintenant_us() - e->t0;

        int reste = (int)(lus / 4);   /* s16 stéréo : 4 octets par échantillon */
        const uint8_t *in = pcm;
        while (reste > 0) {
            int n = swr_convert(a->swr, conv->data, conv->nb_samples, &in, reste);
            if (n <= 0) break;
            reste -= n;
            av_audio_fifo_write(a->fifo, (void **)conv->data, n);
            if (!e->audio_off_pret) {
                e->audio_off = a->emis * 1000000LL / 48000 - contenu;
                e->audio_off_pret = 1;
            }

            /* l'encodeur AAC exige exactement frame_size échantillons */
            while (a->fifo && av_audio_fifo_size(a->fifo) >= a->fs) {
                AVFrame *f = av_frame_alloc();
                f->format = AV_SAMPLE_FMT_FLTP;
                f->sample_rate = 48000;
                av_channel_layout_default(&f->ch_layout, 2);
                f->nb_samples = a->fs;
                f->pts = a->emis * 1000000LL / 48000;
                av_frame_get_buffer(f, 0);
                av_audio_fifo_read(a->fifo, (void **)f->data, a->fs);
                a->emis += a->fs;
                if (avcodec_send_frame(a->enc, f) == 0) {
                    AVPacket *out = av_packet_alloc();
                    while (avcodec_receive_packet(a->enc, out) == 0) {
                        sortie_paquet(e, out, 0, e->audio_tb);
                        av_packet_unref(out);
                    }
                    av_packet_free(&out);
                }
                av_frame_free(&f);
            }
        }
    }
    audio_capture_ferme(a);
    av_free(pcm);
    av_packet_free(&pkt);
    av_frame_free(&conv);
    return NULL;
}

/* ------------------------------------------------------------ socket ctl */

static void mkdir_parent(const char *chemin)
{
    char b[700];
    snprintf(b, sizeof b, "%s", chemin);
    char *p = strrchr(b, '/');
    if (!p) return;
    *p = 0;
    char tmp[700];
    snprintf(tmp, sizeof tmp, "%s", b);
    for (char *q = tmp + 1; *q; q++) {
        if (*q == '/') { *q = 0; mkdir(tmp, 0700); *q = '/'; }
    }
    mkdir(tmp, 0700);
}

static void repondre(int fd, const char *fmt, ...)
{
    char b[1024];
    va_list ap;
    va_start(ap, fmt);
    int n = vsnprintf(b, sizeof b - 2, fmt, ap);
    va_end(ap);
    if (n < 0) return;
    b[n] = '\n';
    (void)!write(fd, b, n + 1);
}

static void statut_json(struct etat *e, char *b, size_t n)
{
    double t = (e->rec_actif && e->rec_debut) ? (maintenant_us() - e->rec_debut) / 1e6 : 0;
    snprintf(b, n,
             "{\"ok\":true,\"recording\":%s,\"replay\":%s,\"replay_sec\":%d,"
             "\"ring\":%.1f,\"file\":\"%s\",\"t\":%.1f,\"frames\":%" PRId64 ","
             "\"pending\":%s,\"err\":%s}",
             e->rec_demande ? "true" : "false", e->replay_demande ? "true" : "false",
             e->cfg.replay_sec, e->ring_sec, e->rec_actif ? e->rec_fichier : "", t, e->frames,
             e->reload_demande ? "true" : "false",
             e->erreur[0] ? "\"\"" : "null");
}

static void *thread_ctl(void *arg)
{
    struct etat *e = arg;
    struct sockaddr_un sa = { .sun_family = AF_UNIX };
    if (strlen(e->cfg.socket) >= sizeof sa.sun_path) {
        erreur(e, "chemin de socket trop long: %s", e->cfg.socket);
        return NULL;
    }
    memcpy(sa.sun_path, e->cfg.socket, strlen(e->cfg.socket) + 1);

    int srv = socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0);
    if (srv < 0) { erreur(e, "socket: %s", strerror(errno)); return NULL; }
    unlink(sa.sun_path);
    if (bind(srv, (struct sockaddr *)&sa, sizeof sa) < 0 || listen(srv, 8) < 0) {
        erreur(e, "bind %s: %s", sa.sun_path, strerror(errno));
        close(srv);
        return NULL;
    }
    e->srv = srv;
    logx("socket de contrôle: %s", sa.sun_path);

    while (!e->arret) {
        int fd = accept(srv, NULL, NULL);
        if (fd < 0) { if (errno == EINTR) continue; break; }
        char ligne[512] = {0};
        ssize_t n = read(fd, ligne, sizeof ligne - 1);
        if (n <= 0) { close(fd); continue; }

        char cmd[64] = {0};
        char *p = strstr(ligne, "\"cmd\"");
        if (p && (p = strchr(p, ':'))) {
            p = strchr(p, '"');
            if (p) {
                p++;
                size_t i = 0;
                while (*p && *p != '"' && i < sizeof cmd - 1) cmd[i++] = *p++;
            }
        }

        char js[1024];
        if (!strcmp(cmd, "status")) {
            statut_json(e, js, sizeof js);
            repondre(fd, "%s", js);
        } else if (!strcmp(cmd, "record.start")) {
            e->rec_fichier[0] = 0;
            e->rec_demande = 1;
            statut_json(e, js, sizeof js);
            repondre(fd, "%s", js);
        } else if (!strcmp(cmd, "record.stop")) {
            e->rec_demande = 0;
            /* répondre seulement quand le mp4 est fermé (moov écrit) : les
             * scripts relisent le fichier juste après, ils doivent le trouver
             * complet. Borne : un encodeur bloqué ne fige pas l'appelant. */
            for (int i = 0; i < 200 && e->rec_actif; i++) usleep(25000);
            statut_json(e, js, sizeof js);
            repondre(fd, "%s", js);
        } else if (!strcmp(cmd, "replay.start")) {
            /* `sec` optionnel : le déclencheur historique passait REPLAY_SEC à
             * chaque appel, la valeur doit suivre sans écriture de config */
            char *ps = strstr(ligne, "\"sec\"");
            if (ps && (ps = strchr(ps, ':'))) {
                int sec = atoi(ps + 1);
                if (sec >= 5 && sec <= 3600) e->cfg.replay_sec = sec;
            }
            e->replay_demande = 1;
            e->auto_lance = 0;          /* choix explicite de l'utilisateur */
            statut_json(e, js, sizeof js);
            repondre(fd, "%s", js);
        } else if (!strcmp(cmd, "replay.stop")) {
            e->replay_demande = 0;
            e->auto_lance = 0;          /* un arrêt manuel coupe l'automatisme */
            statut_json(e, js, sizeof js);
            repondre(fd, "%s", js);
        } else if (!strcmp(cmd, "replay.save")) {
            char f[700] = {0};
            e->sauvegarde = 1;
            int ms = ring_sauve(e, f, sizeof f);
            e->sauvegarde = 0;
            if (ms < 0)
                repondre(fd, "{\"ok\":false,\"err\":\"%s\"}", e->erreur);
            else
                repondre(fd, "{\"ok\":true,\"file\":\"%s\",\"t\":%.1f}", f, ms / 1000.0);
        } else if (!strcmp(cmd, "reload")) {
            /* Jamais au milieu d'un mp4 : couper l'encodeur finaliserait le
             * fichier à l'instant du changement (l'utilisateur perdrait la
             * fin de son enregistrement). On diffère : appliqué au prochain
             * démarrage ou dès que l'enregistrement est fermé. */
            e->reload_demande = 1;
            statut_json(e, js, sizeof js);
            repondre(fd, "%s", js);
        } else if (!strcmp(cmd, "quit")) {
            e->arret = 1;
            repondre(fd, "{\"ok\":true}");
        } else {
            repondre(fd, "{\"ok\":false,\"err\":\"commande inconnue\"}");
        }
        close(fd);
    }
    close(srv);
    e->srv = 0;
    unlink(sa.sun_path);
    return NULL;
}

/* ------------------------------------------------------------------- main */

int main(int argc, char **argv)
{
    static struct etat e;
    memset(&e, 0, sizeof e);
    config_defaut(&e.cfg);
    config_lit_fichier(&e.cfg);

    const char *card = "/dev/dri/card1";
    int secondes = 0, save = 0, daemon_mode = 0;
    for (int i = 1; i < argc; i++) {
        const char *a = argv[i], *v = i + 1 < argc ? argv[i + 1] : "";
        if (!strcmp(a, "--card")) { card = v; i++; }
        else if (!strcmp(a, "--seconds")) { secondes = atoi(v); i++; }
        else if (!strcmp(a, "--out")) { snprintf(e.rec_fichier, sizeof e.rec_fichier, "%s", v); i++; }
        else if (!strcmp(a, "--record")) e.rec_demande = 1;
        else if (!strcmp(a, "--replay")) e.replay_demande = 1;
        else if (!strcmp(a, "--save")) save = 1;
        else if (!strcmp(a, "--tampon")) { snprintf(e.tampon, sizeof e.tampon, "%s", v); i++; }
        else if (!strcmp(a, "--socket")) { snprintf(e.cfg.socket, sizeof e.cfg.socket, "%s", v); i++; }
        else if (!strcmp(a, "--daemon")) daemon_mode = 1;
        else if (!strcmp(a, "--codec")) { snprintf(e.cfg.codec, sizeof e.cfg.codec, "%s", v); i++; }
        else if (!strcmp(a, "--qualite")) { e.cfg.qualite = atoi(v); i++; }
        else if (!strcmp(a, "--fps")) { e.cfg.plafond = !strcmp(v, "flips") ? 0 : atoi(v); i++; }
        else if (!strcmp(a, "--replay-sec")) { e.cfg.replay_sec = atoi(v); i++; }
        else if (!strcmp(a, "--scale")) { sscanf(v, "%dx%d", &e.cfg.res_w, &e.cfg.res_h); i++; }
        else if (!strcmp(a, "--dix-bits")) e.cfg.dix_bits = 1;
        else if (!strcmp(a, "--no-audio")) e.cfg.audio = 0;
        else { fprintf(stderr, "option inconnue: %s\n", a); return 2; }
    }
    config_finalise(&e.cfg);
    if (!e.tampon[0]) snprintf(e.tampon, sizeof e.tampon, "/dev/shm/gpad-rec-%d", (int)getpid());
    mkdir(e.tampon, 0700);
    mkdir(e.cfg.dossier, 0755);
    pthread_mutex_init(&e.verrou, NULL);
    signal(SIGPIPE, SIG_IGN);
    avdevice_register_all();

    if (capture_ouvre(&e.cap, card) < 0) return 1;
    /* pas d'upscale : si l'écran est plus petit que 1080p, on capture natif */
    if (!e.cfg.res_w || e.cfg.res_w > e.cap.w) { e.cfg.res_w = e.cap.w; e.cfg.res_h = e.cap.h; }

    struct audio au;
    memset(&au, 0, sizeof au);
    static struct garde garde;
    e.garde = &garde;
    if (e.cfg.audio && audio_ouvre(&au, &e) == 0) {
        e.audio_actif = 1;
    } else {
        logx("audio désactivé");
    }

    pthread_t tv, ta, tauto;
    pthread_create(&tv, NULL, thread_video, &e);
    if (pthread_create(&tauto, NULL, thread_auto, &e) != 0) tauto = 0;
    if (e.audio_actif) pthread_create(&ta, NULL, thread_audio, &au);

    pthread_t tc;
    int ctl_actif = 0;
    if (daemon_mode || !secondes) {
        mkdir_parent(e.cfg.socket);
        if (pthread_create(&tc, NULL, thread_ctl, &e) == 0) ctl_actif = 1;
    }

    int64_t fin = secondes ? maintenant_us() + (int64_t)secondes * 1000000 : 0;
    while (!e.arret) {
        if (fin && maintenant_us() >= fin) break;
        usleep(50000);
    }
    e.arret = 1;
    if (tauto) pthread_join(tauto, NULL);
    pthread_join(tv, NULL);
    if (e.audio_actif) { au.arret = 1; pthread_join(ta, NULL); }
    if (ctl_actif) { if (e.srv > 0) shutdown(e.srv, SHUT_RDWR); pthread_join(tc, NULL); }

    if (save && e.replay_demande) {
        char f[700] = {0};
        int ms = ring_sauve(&e, f, sizeof f);
        if (ms < 0) logx("sauvegarde replay: échec (%s)", e.erreur);
    }
    logx("%" PRId64 " frames encodées", e.frames);
    return 0;
}
