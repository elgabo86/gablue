/* gpad-shot - capture d'écran par appel direct à KWin (org.kde.KWin.ScreenShot2).
 *
 *   gpad-shot [fichier.png]        (défaut : ~/Images/Screenshots/…)
 *
 * Remplace `spectacle -f -b -n -o` dans `screenshot.sh` : même image (c'est
 * KWin qui compose la frame, on n'écrit que le PNG), sans le client Qt ni son
 * unité systemd. Mesuré sur cette machine (2560x1440, Plasma 6.8.3) :
 * ~30 ms au lieu de ~350 ms.
 *
 * Le POINTEUR N'EST JAMAIS CAPTURÉ : `include-cursor: false` est envoyé
 * explicitement (c'est aussi le défaut de KWin, mais c'est une exigence
 * produit, pas un hasard).
 *
 * Quatre pièges payés, à ne pas réintroduire :
 *  - KWin écrit la frame ENTIÈRE dans le tube AVANT de répondre : sans lecteur
 *    concurrent, le tube (64 Kio) se remplit et les deux côtés s'interbloquent ;
 *  - l'EOF du tube n'est pas fiable (constaté sous Gio/GLib : l'extrémité
 *    d'écriture restait ouverte, `read` bloquait à vie après la frame) : on
 *    s'arrête sur `stride * height` annoncés dans la réponse, avec un délai de
 *    garde ;
 *  - `format` est un QImage::Format : 16 = RGBX8888 (octets R,G,B,X), 4/5 =
 *    ARGB32/BGRA, 13 = RGB888. Les confondre échange rouge et bleu ;
 *  - `CaptureActiveScreen` (l'écran de la fenêtre active, celui du jeu) tombe
 *    en erreur sans fenêtre active : repli sur `CaptureWorkspace`.
 */
#include <dbus/dbus.h>
/* stdio avant jpeglib.h : l'en-tête déclare `jpeg_stdio_dest(FILE *)` et ne
 * tire pas <stdio.h> lui-même (constaté avec libjpeg 9 sur cette machine). */
#include <stdio.h>
#include <jpeglib.h>
#include <setjmp.h>
#include <errno.h>
#include <poll.h>
#include <pthread.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <time.h>
#include <unistd.h>
#include <zlib.h>

#define DELAI_MS 5000  /* appel D-Bus */
#define GARDE_MS 10000 /* attente de la frame dans le tube */

/* --- lecture concurrente du tube ---------------------------------------- */

typedef struct {
    pthread_mutex_t mu;
    pthread_cond_t cv;
    unsigned char *buf;
    size_t len, cap;
    size_t attendu; /* taille annoncée par KWin, 0 tant qu'on ne la connaît pas */
    int fin;        /* EOF vu */
} tube_t;

static void tube_init(tube_t *t)
{
    memset(t, 0, sizeof *t);
    pthread_mutex_init(&t->mu, NULL);
    pthread_cond_init(&t->cv, NULL);
}

static void tube_free(tube_t *t)
{
    free(t->buf);
    t->buf = NULL;
    pthread_mutex_destroy(&t->mu);
    pthread_cond_destroy(&t->cv);
}

static void *lecteur(void *arg)
{
    struct { int fd; tube_t *t; } *a = arg;
    int fd = a->fd;
    tube_t *t = a->t;
    /* délai de garde propre au lecteur : un KWin qui n'écrit plus (ou jamais)
     * ne doit pas laisser la capture bloquée, le script est synchrone */
    struct timespec limite;
    clock_gettime(CLOCK_MONOTONIC, &limite);
    limite.tv_sec += GARDE_MS / 1000;
    for (;;) {
        struct timespec now;
        clock_gettime(CLOCK_MONOTONIC, &now);
        long reste = (limite.tv_sec - now.tv_sec) * 1000L +
                     (limite.tv_nsec - now.tv_nsec) / 1000000L;
        if (reste <= 0)
            break;
        struct pollfd p = {.fd = fd, .events = POLLIN};
        int pret = poll(&p, 1, (int)reste);
        if (pret < 0 && errno == EINTR)
            continue;
        if (pret <= 0)
            break;
        unsigned char tmp[65536];
        ssize_t r = read(fd, tmp, sizeof tmp);
        if (r < 0 && errno == EINTR)
            continue;
        if (r <= 0)
            break;
        pthread_mutex_lock(&t->mu);
        if (t->len + (size_t)r > t->cap) {
            while (t->len + (size_t)r > t->cap)
                t->cap = t->cap ? t->cap * 2 : (1u << 22);
            unsigned char *n = realloc(t->buf, t->cap);
            if (!n) { /* hors mémoire : on laisse le producteur se bloquer,
                        l'attente de la frame rendra la main par le délai */
                pthread_mutex_unlock(&t->mu);
                return NULL;
            }
            t->buf = n;
        }
        memcpy(t->buf + t->len, tmp, (size_t)r);
        t->len += (size_t)r;
        int atteint = t->attendu && t->len >= t->attendu;
        pthread_cond_broadcast(&t->cv);
        pthread_mutex_unlock(&t->mu);
        if (atteint)
            return NULL;
    }
    pthread_mutex_lock(&t->mu);
    t->fin = 1;
    pthread_cond_broadcast(&t->cv);
    pthread_mutex_unlock(&t->mu);
    return NULL;
}

/* --- réponse D-Bus : a{sv} -> dimensions -------------------------------- */

typedef struct {
    uint32_t w, h, stride, format;
} capture_t;

static int lire_reponse(DBusMessage *rep, capture_t *c)
{
    DBusMessageIter it, arr;
    if (!dbus_message_iter_init(rep, &it) ||
        dbus_message_iter_get_arg_type(&it) != DBUS_TYPE_ARRAY)
        return -1;
    dbus_message_iter_recurse(&it, &arr);
    while (dbus_message_iter_get_arg_type(&arr) == DBUS_TYPE_DICT_ENTRY) {
        DBusMessageIter de, v;
        const char *cle = NULL;
        dbus_message_iter_recurse(&arr, &de);
        dbus_message_iter_get_basic(&de, &cle);
        dbus_message_iter_next(&de);
        dbus_message_iter_recurse(&de, &v);
        uint32_t *cible = NULL;
        if (!strcmp(cle, "width")) cible = &c->w;
        else if (!strcmp(cle, "height")) cible = &c->h;
        else if (!strcmp(cle, "stride")) cible = &c->stride;
        else if (!strcmp(cle, "format")) cible = &c->format;
        if (cible && dbus_message_iter_get_arg_type(&v) == DBUS_TYPE_UINT32)
            dbus_message_iter_get_basic(&v, cible);
        dbus_message_iter_next(&arr);
    }
    return (c->w && c->h && c->stride) ? 0 : -1;
}

/* options communes : pas de pointeur, résolution physique (comme Spectacle) */
static void ajouter_options(DBusMessageIter *it)
{
    static const char *const noms[] = {"include-cursor", "native-resolution"};
    static const dbus_bool_t vals[] = {FALSE, TRUE};
    DBusMessageIter arr;
    dbus_message_iter_open_container(it, DBUS_TYPE_ARRAY, "{sv}", &arr);
    for (size_t i = 0; i < sizeof noms / sizeof *noms; i++) {
        DBusMessageIter de, v;
        dbus_message_iter_open_container(&arr, DBUS_TYPE_DICT_ENTRY, NULL, &de);
        dbus_message_iter_append_basic(&de, DBUS_TYPE_STRING, &noms[i]);
        dbus_message_iter_open_container(&de, DBUS_TYPE_VARIANT, "b", &v);
        dbus_message_iter_append_basic(&v, DBUS_TYPE_BOOLEAN, &vals[i]);
        dbus_message_iter_close_container(&de, &v);
        dbus_message_iter_close_container(&arr, &de);
    }
    dbus_message_iter_close_container(it, &arr);
}

/* un appel de capture complet ; buf rendu au demandeur (jamais libéré ici) */
static int capture(DBusConnection *conn, const char *methode,
                   capture_t *c, unsigned char **buf, size_t *len)
{
    int pfd[2];
    if (pipe(pfd) < 0) {
        fprintf(stderr, "gpad-shot: tube: %s\n", strerror(errno));
        return -1;
    }
    tube_t t;
    tube_init(&t);
    struct { int fd; tube_t *t; } larg = {pfd[0], &t};
    pthread_t th;
    if (pthread_create(&th, NULL, lecteur, &larg) != 0) {
        close(pfd[0]);
        close(pfd[1]);
        tube_free(&t);
        fprintf(stderr, "gpad-shot: thread: %s\n", strerror(errno));
        return -1;
    }

    DBusMessage *m = dbus_message_new_method_call(
        "org.kde.KWin", "/org/kde/KWin/ScreenShot2",
        "org.kde.KWin.ScreenShot2", methode);
    DBusMessageIter it;
    dbus_message_iter_init_append(m, &it);
    ajouter_options(&it);
    dbus_message_iter_append_basic(&it, DBUS_TYPE_UNIX_FD, &pfd[1]);
    close(pfd[1]);

    DBusError err;
    dbus_error_init(&err);
    DBusMessage *rep =
        dbus_connection_send_with_reply_and_block(conn, m, DELAI_MS, &err);
    dbus_message_unref(m);
    if (!rep) {
        fprintf(stderr, "gpad-shot: %s: %s\n", methode,
                err.message ? err.message : "appel refusé");
        dbus_error_free(&err);
        pthread_join(th, NULL);
        close(pfd[0]);
        tube_free(&t);
        return -1;
    }

    int rc = -1;
    if (lire_reponse(rep, c) < 0) {
        fprintf(stderr, "gpad-shot: %s: réponse illisible\n", methode);
        goto sortie;
    }
    if (c->format != 13 && c->format != 16 && c->format != 17 &&
        c->format != 18 && c->format != 4 && c->format != 5) {
        fprintf(stderr, "gpad-shot: QImage::Format %u non géré\n", c->format);
        goto sortie;
    }
    size_t attendu = (size_t)c->stride * c->h;

    /* la frame arrive dans le tube pendant l'appel ; on attend la fin */
    struct timespec limite;
    clock_gettime(CLOCK_REALTIME, &limite);
    limite.tv_sec += DELAI_MS / 1000;
    pthread_mutex_lock(&t.mu);
    t.attendu = attendu;
    while (t.len < attendu && !t.fin) {
        if (pthread_cond_timedwait(&t.cv, &t.mu, &limite) == ETIMEDOUT)
            break;
    }
    size_t recu = t.len;
    unsigned char *data = t.buf;
    t.buf = NULL;
    pthread_mutex_unlock(&t.mu);

    if (recu < attendu) {
        fprintf(stderr, "gpad-shot: %s: frame incomplète (%zu/%zu)\n", methode,
                recu, attendu);
        free(data);
        goto sortie;
    }
    *buf = data;
    *len = recu;
    rc = 0;

sortie:
    dbus_message_unref(rep);
    pthread_join(th, NULL);
    close(pfd[0]);
    tube_free(&t);
    return rc;
}

/* --- écriture PNG (filtre 0, deflate niveau 1) --------------------------- */
/* Pas de libpng : zlib suffit, et le niveau 1 garde l'encodage sous ~20 ms
 * (le niveau 6 de Spectacle coûte plus cher sans gain utile). */

static int ecrire_chunk(FILE *f, const char *type, const unsigned char *data,
                        size_t len)
{
    unsigned char entete[8] = {(unsigned char)(len >> 24),
                               (unsigned char)(len >> 16),
                               (unsigned char)(len >> 8),
                               (unsigned char)len};
    memcpy(entete + 4, type, 4);
    uLong c = crc32(0, entete + 4, 4);
    if (len)
        c = crc32(c, data, (uInt)len);
    unsigned char fin[4] = {(unsigned char)(c >> 24), (unsigned char)(c >> 16),
                            (unsigned char)(c >> 8), (unsigned char)c};
    return fwrite(entete, 1, 8, f) == 8 &&
           (!len || fwrite(data, 1, len, f) == len) && fwrite(fin, 1, 4, f) == 4;
}

/* iTXt (UTF-8, non compressé) : clé + texte lisible par `identify`, `exiftool`
 * et la galerie du launcher. tEXt serait Latin-1 : un titre accentué ou
 * japonais y serait corrompu, d'où l'iTXt. */
static int ecrire_itxt(FILE *f, const char *cle, const char *texte)
{
    size_t lc = strlen(cle), lt = strlen(texte);
    size_t len = lc + 1 + 1 + 1 + 1 + 1 + lt; /* clé\0 flag méthode lang\0 trad\0 texte */
    unsigned char *d = calloc(1, len);
    if (!d)
        return 0;
    unsigned char *p = d;
    memcpy(p, cle, lc);
    p += lc + 1;         /* clé + NUL */
    *p++ = 0;            /* compression : aucune */
    *p++ = 0;            /* méthode */
    *p++ = 0;            /* langue : vide */
    *p++ = 0;            /* mot-clé traduit : vide */
    memcpy(p, texte, lt);
    int ok = ecrire_chunk(f, "iTXt", d, len);
    free(d);
    return ok;
}

static int ecrire_png(const char *chemin, const capture_t *c,
                      const unsigned char *raw, size_t len, const char *titre,
                      int niveau)
{
    const int bpp = (c->format == 13) ? 3 : 4;
    /* QImage::Format 16/17/18 = octets R,V,B(,A) ; 4/5 et 13 = B,V,R */
    const int i0 = (c->format >= 16) ? 0 : 2;
    const int i2 = (c->format >= 16) ? 2 : 0;
    const size_t ligne = (size_t)c->w * 3 + 1;
    size_t total = ligne * c->h;
    if ((size_t)c->stride * c->h > len || c->stride < (size_t)c->w * bpp) {
        fprintf(stderr, "gpad-shot: frame incohérente\n");
        return -1;
    }

    unsigned char *rgb = malloc(total);
    unsigned char *z = NULL;
    if (!rgb)
        return -1;
    for (uint32_t y = 0; y < c->h; y++) {
        const unsigned char *s = raw + (size_t)y * c->stride;
        unsigned char *d = rgb + (size_t)y * ligne;
        *d++ = 0;
        for (uint32_t x = 0; x < c->w; x++, s += bpp) {
            d[0] = s[i0];
            d[1] = s[1];
            d[2] = s[i2];
            d += 3;
        }
    }

    uLongf zlen = compressBound(total);
    z = malloc(zlen);
    if (!z || compress2(z, &zlen, rgb, total, niveau) != Z_OK) {
        free(rgb);
        free(z);
        return -1;
    }
    free(rgb);

    FILE *f = fopen(chemin, "wb");
    if (!f) {
        fprintf(stderr, "gpad-shot: %s: %s\n", chemin, strerror(errno));
        free(z);
        return -1;
    }
    static const unsigned char sig[8] = {0x89, 'P', 'N', 'G', 0x0d, 0x0a, 0x1a, 0x0a};
    unsigned char ihdr[13] = {(unsigned char)(c->w >> 24),
                              (unsigned char)(c->w >> 16),
                              (unsigned char)(c->w >> 8),
                              (unsigned char)c->w,
                              (unsigned char)(c->h >> 24),
                              (unsigned char)(c->h >> 16),
                              (unsigned char)(c->h >> 8),
                              (unsigned char)c->h,
                              8, 2, 0, 0, 0}; /* 8 bits RVB, pas d'entrelacement */
    int ok = fwrite(sig, 1, 8, f) == 8 &&
             ecrire_chunk(f, "IHDR", ihdr, sizeof ihdr) &&
             (!titre || !*titre || ecrire_itxt(f, "Title", titre)) &&
             ecrire_chunk(f, "IDAT", z, (size_t)zlen) &&
             ecrire_chunk(f, "IEND", NULL, 0);
    free(z);
    if (fclose(f) != 0 || !ok) {
        fprintf(stderr, "gpad-shot: écriture de %s échouée\n", chemin);
        return -1;
    }
    return 0;
}

/* --- réglages de capture (recorder.conf) ---------------------------------- */
/* Le MÊME fichier que l'overlay (page CAPTURE) et Gablue Manager : une seule
 * source de vérité, aucun réglage recopié ailleurs. Fichier absent ou clé
 * inconnue = défaut, jamais une erreur (une capture qui échoue pour un réglage
 * manquant serait pire que le réglage ignoré). */
static void conf_lire(const char *cle, char *sortie, size_t taille,
                      const char *defaut)
{
    snprintf(sortie, taille, "%s", defaut);
    const char *home = getenv("HOME");
    if (!home || !*home)
        return;
    char chemin[4096];
    snprintf(chemin, sizeof chemin, "%s/.config/gablue/recorder.conf", home);
    FILE *f = fopen(chemin, "r");
    if (!f)
        return;
    char ligne[512];
    while (fgets(ligne, sizeof ligne, f)) {
        char *p = ligne;
        while (*p == ' ' || *p == '\t')
            p++;
        if (*p == '#' || *p == '\n' || *p == 0)
            continue;
        size_t n = strlen(cle);
        if (strncmp(p, cle, n) != 0)
            continue;
        p += n;
        while (*p == ' ' || *p == '\t')
            p++;
        if (*p != '=')
            continue;
        p++;
        while (*p == ' ' || *p == '\t')
            p++;
        char *fin = p + strlen(p);
        while (fin > p && (fin[-1] == '\n' || fin[-1] == '\r' || fin[-1] == ' ' ||
                           fin[-1] == '\t'))
            fin--;
        size_t len = (size_t)(fin - p);
        if (len >= taille)
            len = taille - 1;
        memcpy(sortie, p, len);
        sortie[len] = 0;
    }
    fclose(f);
}

/* --- écriture JPEG (libjpeg-turbo) --------------------------------------- */
/* Sans gestionnaire d'erreur, libjpeg appelle exit() à la moindre erreur : un
 * disque plein tuerait la capture ET la session qui l'attend. `longjmp` rend
 * la main proprement. */
struct jpeg_erreur {
    struct jpeg_error_mgr mgr;
    jmp_buf saut;
};

static void jpeg_abandon(j_common_ptr cinfo)
{
    struct jpeg_erreur *e = (struct jpeg_erreur *)cinfo->err;
    char msg[JMSG_LENGTH_MAX];
    (*cinfo->err->format_message)(cinfo, msg);
    fprintf(stderr, "gpad-shot: jpeg: %s\n", msg);
    longjmp(e->saut, 1);
}

static int ecrire_jpeg(const char *chemin, const capture_t *c,
                       const unsigned char *raw, size_t len, const char *titre,
                       int qualite)
{
    /* `volatile` : ces trois valeurs sont lues après un `longjmp` (erreur
     * libjpeg), et le compilateur a le droit de les garder en registre sans
     * ça (-Wclobbered). */
    const volatile int bpp = (c->format == 13) ? 3 : 4;
    const volatile int i0 = (c->format >= 16) ? 0 : 2;
    const volatile int i2 = (c->format >= 16) ? 2 : 0;
    if ((size_t)c->stride * c->h > len || c->stride < (size_t)c->w * bpp) {
        fprintf(stderr, "gpad-shot: frame incohérente\n");
        return -1;
    }
    unsigned char *ligne = malloc((size_t)c->w * 3);
    if (!ligne)
        return -1;

    FILE *f = fopen(chemin, "wb");
    if (!f) {
        fprintf(stderr, "gpad-shot: %s: %s\n", chemin, strerror(errno));
        free(ligne);
        return -1;
    }

    struct jpeg_compress_struct cinfo;
    struct jpeg_erreur jerr;
    cinfo.err = jpeg_std_error(&jerr.mgr);
    jerr.mgr.error_exit = jpeg_abandon;
    if (setjmp(jerr.saut)) {
        jpeg_destroy_compress(&cinfo);
        fclose(f);
        remove(chemin); /* fichier tronqué : ne pas le laisser passer pour une capture */
        free(ligne);
        return -1;
    }
    jpeg_create_compress(&cinfo);
    jpeg_stdio_dest(&cinfo, f);
    cinfo.image_width = c->w;
    cinfo.image_height = c->h;
    cinfo.input_components = 3;
    cinfo.in_color_space = JCS_RGB;
    jpeg_set_defaults(&cinfo);
    jpeg_set_quality(&cinfo, qualite, TRUE);
    /* 4:2:0 suffit pour une capture de jeu et allège nettement ; le 4:4:4 de
     * libjpeg par défaut gardait 1,5× le poids pour un gain invisible. */
    cinfo.comp_info[0].h_samp_factor = 2;
    cinfo.comp_info[0].v_samp_factor = 2;
    jpeg_start_compress(&cinfo, TRUE);
    if (titre && *titre)
        jpeg_write_marker(&cinfo, JPEG_COM, (const JOCTET *)titre,
                          (unsigned int)strlen(titre));
    while (cinfo.next_scanline < cinfo.image_height) {
        const unsigned char *s =
            raw + (size_t)cinfo.next_scanline * c->stride;
        unsigned char *d = ligne;
        for (uint32_t x = 0; x < c->w; x++, s += bpp) {
            d[0] = s[i0];
            d[1] = s[1];
            d[2] = s[i2];
            d += 3;
        }
        JSAMPROW rangees[1] = {ligne};
        jpeg_write_scanlines(&cinfo, rangees, 1);
    }
    jpeg_finish_compress(&cinfo);
    jpeg_destroy_compress(&cinfo);
    free(ligne);
    if (fclose(f) != 0) {
        fprintf(stderr, "gpad-shot: écriture de %s échouée\n", chemin);
        return -1;
    }
    return 0;
}

/* mkdir -p minimal (screenshot.sh le fait aussi, on ne dépend de personne) */
static void mkdir_p(const char *chemin)
{
    char tmp[4096];
    snprintf(tmp, sizeof tmp, "%s", chemin);
    for (char *p = tmp + 1; *p; p++) {
        if (*p != '/')
            continue;
        *p = '\0';
        mkdir(tmp, 0755);
        *p = '/';
    }
    mkdir(tmp, 0755);
}

static char *chemin_defaut(const char *ext)
{
    const char *home = getenv("HOME");
    const char *dir = getenv("SCREENSHOT_DIR");
    char dossier[4096];
    if (dir && *dir)
        snprintf(dossier, sizeof dossier, "%s", dir);
    else if (home && *home)
        snprintf(dossier, sizeof dossier, "%s/Images/Screenshots", home);
    else {
        fprintf(stderr, "gpad-shot: HOME absent\n");
        return NULL;
    }
    char *c = malloc(sizeof dossier + 64);
    if (!c)
        return NULL;
    mkdir_p(dossier);
    time_t now = time(NULL);
    struct tm tm;
    localtime_r(&now, &tm);
    char date[32];
    strftime(date, sizeof date, "%Y%m%d_%H%M%S", &tm);
    snprintf(c, sizeof dossier + 64, "%s/screenshot_%s.%s", dossier, date, ext);
    return c;
}

int main(int argc, char **argv)
{
    /* --titre=<texte> : nom du jeu (ou de la fenêtre) gravé en métadonnée
     * iTXt, jamais dans les pixels (demandé le 23/11). */
    const char *titre = getenv("GPAD_SHOT_TITRE");
    const char *fichier = NULL;
    for (int i = 1; i < argc; i++) {
        if (strncmp(argv[i], "--titre=", 8) == 0)
            titre = argv[i] + 8;
        else if (!fichier)
            fichier = argv[i];
        else {
            fprintf(stderr, "usage: gpad-shot [--titre=<texte>] [fichier.png]\n");
            return 2;
        }
    }
    /* format et qualité : recorder.conf, écrits par l'overlay (page CAPTURE)
     * et par Gablue Manager. JPEG par défaut — une capture PNG 1440p pesait
     * plusieurs Mo (signalé le 23/11/2026). */
    char format[16];
    char qualite[16];
    conf_lire("shot_format", format, sizeof format, "jpeg");
    conf_lire("shot_qualite", qualite, sizeof qualite, "haute");
    const int jpeg = strcmp(format, "png") != 0;
    int niveau = 6;
    int q = 92;
    if (strcmp(qualite, "basse") == 0) {
        niveau = 3;
        q = 72;
    } else if (strcmp(qualite, "moyenne") == 0) {
        niveau = 6;
        q = 82;
    } else {
        niveau = 9;
        q = 92;
    }

    char *chemin = fichier ? strdup(fichier) : chemin_defaut(jpeg ? "jpg" : "png");
    if (!chemin) {
        fprintf(stderr, "usage: gpad-shot [--titre=<texte>] [fichier.png]\n");
        return 2;
    }

    dbus_threads_init_default();
    DBusError err;
    dbus_error_init(&err);
    DBusConnection *conn = dbus_bus_get(DBUS_BUS_SESSION, &err);
    if (!conn) {
        fprintf(stderr, "gpad-shot: session D-Bus: %s\n",
                err.message ? err.message : "absente");
        dbus_error_free(&err);
        free(chemin);
        return 1;
    }

    /* un écran (celui du jeu) ; tout le bureau seulement s'il n'y a pas de
     * fenêtre active, ce qui ferait échouer CaptureActiveScreen. */
    capture_t c;
    unsigned char *buf = NULL;
    size_t len = 0;
    if (capture(conn, "CaptureActiveScreen", &c, &buf, &len) < 0 &&
        capture(conn, "CaptureWorkspace", &c, &buf, &len) < 0) {
        dbus_connection_unref(conn);
        free(chemin);
        return 1;
    }

    int rc = jpeg ? ecrire_jpeg(chemin, &c, buf, len, titre, q)
                  : ecrire_png(chemin, &c, buf, len, titre, niveau);
    fprintf(stderr, "gpad-shot: %s (%s, qualite %s)\n", chemin,
            jpeg ? "jpeg" : "png", qualite);
    free(buf);
    dbus_connection_unref(conn);
    free(chemin);
    return rc < 0 ? 1 : 0;
}
