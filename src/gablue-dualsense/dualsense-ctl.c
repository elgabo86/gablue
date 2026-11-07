// SPDX-License-Identifier: Apache-2.0
/*
 * gablue-dualsense-ctl — lecture d'état et mise à jour du firmware des
 * manettes DualSense / DualSense Edge sous Linux.
 *
 * Protocole : rapports HID « feature » 0xF4 (commande) / 0xF5 (statut), et
 * rapport 0x20 (informations firmware). Mise à jour en USB uniquement.
 *
 * Deux commandes, sortie lisible par un script (clé=valeur) :
 *   info            état de la manette
 *   flash FICHIER   écrit un firmware (image Sony de 950272 octets)
 *
 * Aucun privilège : l'accès à /dev/hidraw* vient de la règle udev uaccess
 * (ublue-os-udev-rules / steam-devices, déjà présentes dans Gablue).
 */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>
#include <unistd.h>
#include <time.h>
#include <hidapi/hidapi.h>

#define DS_VID 0x054c
#define DS_PID 0x0ce6
#define DSE_PID 0x0df2

#define REPORT_INFO 0x20
#define REPORT_FW 0xF4
#define REPORT_FW_STATUS 0xF5

#define REPORT_SIZE 64
#define FW_SIZE 950272
#define BATTERY_MIN 10

/* Codes de sortie (documentés dans l'aide et dans le script) */
enum {
    RC_OK = 0,
    RC_USAGE = 1,
    RC_NO_DEVICE = 3,
    RC_BLUETOOTH = 4,
    RC_BATTERY = 5,
    RC_BAD_IMAGE = 6,
    RC_IO = 7,
    RC_REJECTED = 8,
    RC_VERIFY = 9,
};

struct device {
    hid_device *h;
    uint16_t pid;
    int bt;
    char serial[64];
};

struct fw_info {
    char build_date[12];
    char build_time[9];
    uint16_t fw_type;
    uint16_t sw_series;
    uint32_t hardware;
    uint32_t version;
    uint16_t update_version;
};

static void sleep_ms(int ms)
{
    struct timespec ts = { .tv_sec = ms / 1000, .tv_nsec = (long)(ms % 1000) * 1000000L };
    nanosleep(&ts, NULL);
}

/* Ouvre la première manette trouvée, en préférant l'USB. */
static int open_device(struct device *d)
{
    struct hid_device_info *list, *it, *usb = NULL, *any = NULL;

    memset(d, 0, sizeof(*d));
    if (hid_init() != 0) {
        fprintf(stderr, "hidapi : initialisation impossible\n");
        return -1;
    }

    list = hid_enumerate(DS_VID, DS_PID);
    struct hid_device_info *edge = hid_enumerate(DS_VID, DSE_PID);
    if (edge) {
        if (list) {
            it = list;
            while (it->next) it = it->next;
            it->next = edge;
        } else {
            list = edge;
        }
    }

    for (it = list; it; it = it->next) {
        if (it->interface_number >= 0) {
            if (!usb) usb = it;
        } else if (!any) {
            any = it;
        }
    }

    struct hid_device_info *pick = usb ? usb : any;
    if (!pick) {
        hid_free_enumeration(list);
        hid_exit();
        return -1;
    }

    d->h = hid_open_path(pick->path);
    d->pid = pick->product_id;
    d->bt = pick->interface_number < 0;
    if (pick->serial_number) {
        size_t i;
        for (i = 0; i < sizeof(d->serial) - 1 && pick->serial_number[i]; i++)
            d->serial[i] = (char)pick->serial_number[i];
        d->serial[i] = '\0';
    }
    hid_free_enumeration(list);

    if (!d->h) {
        fprintf(stderr, "hidapi : ouverture impossible (%ls)\n", hid_error(NULL));
        hid_exit();
        return -1;
    }
    return 0;
}

static void close_device(struct device *d)
{
    if (d->h) hid_close(d->h);
    d->h = NULL;
    hid_exit();
}

static int read_fw_info(struct device *d, struct fw_info *fi)
{
    uint8_t buf[REPORT_SIZE];
    int r;

    memset(fi, 0, sizeof(*fi));
    memset(buf, 0, sizeof(buf));
    buf[0] = REPORT_INFO;
    r = hid_get_feature_report(d->h, buf, sizeof(buf));
    if (r != REPORT_SIZE) {
        fprintf(stderr, "rapport 0x20 illisible (%d octets)\n", r);
        return -1;
    }
    memcpy(fi->build_date, buf + 1, 11);
    memcpy(fi->build_time, buf + 12, 8);
    fi->fw_type = (uint16_t)(buf[20] | (buf[21] << 8));
    fi->sw_series = (uint16_t)(buf[22] | (buf[23] << 8));
    fi->hardware = (uint32_t)buf[24] | ((uint32_t)buf[25] << 8) |
                   ((uint32_t)buf[26] << 16) | ((uint32_t)buf[27] << 24);
    fi->version = (uint32_t)buf[28] | ((uint32_t)buf[29] << 8) |
                  ((uint32_t)buf[30] << 16) | ((uint32_t)buf[31] << 24);
    fi->update_version = (uint16_t)(buf[44] | (buf[45] << 8));
    return 0;
}

/* Lit un rapport d'entrée et renvoie capacité + état de charge. */
static int read_battery(struct device *d, int *capacity, const char **state)
{
    uint8_t data[78];
    int r = hid_read_timeout(d->h, data, sizeof(data), 1000);
    size_t off;
    uint8_t status, cap, charging;

    if (r <= 0) return -1;

    if (!d->bt && data[0] == 0x01 && r == REPORT_SIZE) {
        off = 1;
    } else if (d->bt && data[0] == 0x31) {
        off = 2;
    } else {
        return -1;
    }

    status = data[off + 52];
    cap = status & 0x0f;
    charging = (status & 0xf0) >> 4;

    switch (charging) {
    case 0x0:
        *capacity = cap * 10 + 5 > 100 ? 100 : cap * 10 + 5;
        *state = "decharge";
        break;
    case 0x1:
        *capacity = cap * 10 + 5 > 100 ? 100 : cap * 10 + 5;
        *state = "charge";
        break;
    case 0x2:
        *capacity = 100;
        *state = "pleine";
        break;
    case 0xa:
    case 0xb:
        *capacity = 0;
        *state = "hors-plage";
        break;
    default:
        *capacity = 0;
        *state = "inconnu";
    }
    return 0;
}

static int cmd_info(void)
{
    struct device d;
    struct fw_info fi;
    int capacity = -1;
    const char *bstate = "inconnu";
    int rc = RC_OK;

    if (open_device(&d) != 0) {
        printf("CONNECTED=no\n");
        return RC_NO_DEVICE;
    }

    printf("CONNECTED=%s\n", d.bt ? "bluetooth" : "usb");
    printf("MODEL=%s\n", d.pid == DS_PID ? "dualsense" : "edge");
    printf("PID=%04x\n", d.pid);
    printf("SERIAL=%s\n", d.serial);

    if (read_fw_info(&d, &fi) != 0) {
        rc = RC_IO;
    } else {
        printf("VERSION=%04x\n", fi.update_version);
        printf("FIRMWARE_VERSION=%08x\n", fi.version);
        printf("BUILD_DATE=%s\n", fi.build_date);
        printf("BUILD_TIME=%s\n", fi.build_time);
        printf("HARDWARE=%04x\n", fi.hardware);
        printf("FW_TYPE=%u\n", fi.fw_type);
        printf("SW_SERIES=%u\n", fi.sw_series);
    }

    if (read_battery(&d, &capacity, &bstate) == 0) {
        printf("BATTERY=%d\n", capacity);
        printf("BATTERY_STATE=%s\n", bstate);
    }

    close_device(&d);
    return rc;
}

static int send_cmd(struct device *d, const uint8_t *buf)
{
    int r = hid_send_feature_report(d->h, buf, REPORT_SIZE);
    if (r != REPORT_SIZE) {
        fprintf(stderr, "envoi du rapport firmware impossible (%d)\n", r);
        return -1;
    }
    return 0;
}

/*
 * Attend un statut du phase donné. Renvoie 0 si le code 0 (ou 3 en écriture,
 * « déjà appliqué ») est atteint, -2 si le matériel refuse l'image, -1 sinon.
 * 0x01/0x10 = occupé, on réessaie ; 0x04 = traitement long.
 */
static int wait_status(struct device *d, uint8_t phase, int timeout_ms, int accept3)
{
    uint8_t buf[REPORT_SIZE];
    int elapsed = 0;

    while (elapsed <= timeout_ms) {
        int r;
        uint8_t st_phase, code;

        memset(buf, 0, sizeof(buf));
        buf[0] = REPORT_FW_STATUS;
        r = hid_get_feature_report(d->h, buf, sizeof(buf));
        if (r < 0) {
            fprintf(stderr, "statut firmware illisible (%ls)\n", hid_error(d->h));
            return -1;
        }
        st_phase = buf[1];
        code = buf[2];

        if (st_phase == phase) {
            switch (code) {
            case 0x00:
                return 0;
            case 0x01:
            case 0x10:
                break;
            case 0x02:
                fprintf(stderr, "image refusée : la manette n'accepte qu'une version plus récente (ou image incompatible)\n");
                return -2;
            case 0x03:
                if (accept3) return 0;
                fprintf(stderr, "image invalide\n");
                return -2;
            case 0x04:
                sleep_ms(1000);
                break;
            case 0x11:
                fprintf(stderr, "image invalide (contrôle matériel)\n");
                return -2;
            case 0xff:
                fprintf(stderr, "erreur interne du firmware de la manette\n");
                return -2;
            default:
                fprintf(stderr, "statut firmware inconnu 0x%02x\n", code);
                return -2;
            }
        }
        sleep_ms(20);
        elapsed += 20;
    }
    fprintf(stderr, "délai dépassé en attente du statut 0x%02x\n", phase);
    return -1;
}

static uint8_t *load_image(const char *path, size_t *size, uint16_t *pid, uint32_t *target, uint16_t *version)
{
    FILE *f = fopen(path, "rb");
    uint8_t *buf;

    if (!f) {
        perror(path);
        return NULL;
    }
    fseek(f, 0, SEEK_END);
    *size = (size_t)ftell(f);
    rewind(f);
    if (*size != FW_SIZE) {
        fprintf(stderr, "taille inattendue : %zu octets (attendu %d)\n", *size, FW_SIZE);
        fclose(f);
        return NULL;
    }
    buf = malloc(*size);
    if (!buf) {
        fclose(f);
        return NULL;
    }
    if (fread(buf, 1, *size, f) != *size) {
        fprintf(stderr, "lecture incomplète de %s\n", path);
        free(buf);
        fclose(f);
        return NULL;
    }
    fclose(f);

    *pid = (uint16_t)(buf[0x62] | (buf[0x63] << 8));
    *target = (uint32_t)buf[0x74] | ((uint32_t)buf[0x75] << 8) |
              ((uint32_t)buf[0x76] << 16) | ((uint32_t)buf[0x77] << 24);
    *version = (uint16_t)(buf[0x78] | (buf[0x79] << 8));
    return buf;
}

static int cmd_flash(const char *path)
{
    struct device d;
    struct fw_info fi;
    size_t size = 0;
    uint16_t img_pid = 0, img_version = 0;
    uint32_t img_target = 0;
    uint8_t *fw;
    uint8_t buf[REPORT_SIZE];
    size_t offset;
    int capacity = -1;
    int batt_ok = 0;
    const char *bstate = "inconnu";

    if (open_device(&d) != 0) {
        fprintf(stderr, "aucune manette détectée\n");
        return RC_NO_DEVICE;
    }
    if (d.bt) {
        fprintf(stderr, "mise à jour impossible en Bluetooth : branchez la manette en USB\n");
        close_device(&d);
        return RC_BLUETOOTH;
    }
    if (read_fw_info(&d, &fi) != 0) {
        close_device(&d);
        return RC_IO;
    }

    batt_ok = read_battery(&d, &capacity, &bstate) == 0;
    if (batt_ok && capacity < BATTERY_MIN) {
        fprintf(stderr, "batterie trop faible (%d %%, minimum %d %%) : chargez la manette\n",
                capacity, BATTERY_MIN);
        close_device(&d);
        return RC_BATTERY;
    }

    fw = load_image(path, &size, &img_pid, &img_target, &img_version);
    if (!fw) {
        close_device(&d);
        return RC_BAD_IMAGE;
    }
    if (img_pid != d.pid) {
        fprintf(stderr, "firmware pour la manette 0x%04x, manette connectée 0x%04x\n", img_pid, d.pid);
        free(fw);
        close_device(&d);
        return RC_BAD_IMAGE;
    }

    printf("Manette      : %s\n", d.pid == DS_PID ? "DualSense" : "DualSense Edge");
    printf("Firmware     : %04x -> %04x (cible %04x)\n", fi.update_version, img_version, img_target);
    if (batt_ok) {
        printf("Batterie     : %d %% (%s)\n", capacity, bstate);
    } else {
        printf("Batterie     : inconnue (lecture impossible)\n");
    }
    fflush(stdout);

    printf("Envoi de l'en-tête…\n");
    fflush(stdout);
    for (offset = 0; offset < 256; offset += 57) {
        size_t chunk = (256 - offset < 57) ? 256 - offset : 57;
        memset(buf, 0, sizeof(buf));
        buf[0] = REPORT_FW;
        buf[2] = (uint8_t)chunk;
        memcpy(buf + 3, fw + offset, chunk);
        if (send_cmd(&d, buf) != 0) {
            free(fw);
            close_device(&d);
            return RC_IO;
        }
        if (offset == 0) sleep_ms(50);
    }
    {
        int r = wait_status(&d, 0x00, 5000, 0);
        if (r == -2) {
            free(fw);
            close_device(&d);
            return RC_REJECTED;
        }
        /* -1 (délai) : la manette reste en 0x01 « prête à recevoir », on écrit ;
         * c'est le comportement observé sur BDM-020/030 (firmware 0x0630). */
        if (r != 0)
            printf("En-tête accepté, attente de données…\n");
    }

    printf("Écriture du firmware…\n");
    fflush(stdout);
    for (offset = 0; offset < size; offset += 0x8000) {
        size_t chunk_offset;
        for (chunk_offset = 0; chunk_offset < 0x8000; chunk_offset += 57) {
            size_t remaining = 0x8000 - chunk_offset;
            size_t packet = remaining < 57 ? remaining : 57;
            int progress;

            memset(buf, 0, sizeof(buf));
            buf[0] = REPORT_FW;
            buf[1] = 0x01;
            buf[2] = (uint8_t)packet;
            memcpy(buf + 3, fw + offset + chunk_offset, packet);
            if (send_cmd(&d, buf) != 0 || wait_status(&d, 0x01, 5000, 1) != 0) {
                fprintf(stderr, "\nécriture interrompue à l'octet %zu\n", offset + chunk_offset);
                free(fw);
                close_device(&d);
                return RC_IO;
            }
            sleep_ms(10);

            progress = (int)((offset + chunk_offset - 256) * 100 / (size - 256));
            if (progress < 0) progress = 0;
            if (progress > 100) progress = 100;
            printf("\r  %3d %%", progress);
            fflush(stdout);
        }
    }
    printf("\r  100 %%\n");

    printf("Vérification…\n");
    fflush(stdout);
    memset(buf, 0, sizeof(buf));
    buf[0] = REPORT_FW;
    buf[1] = 0x02;
    if (send_cmd(&d, buf) != 0) {
        free(fw);
        close_device(&d);
        return RC_IO;
    }
    if (wait_status(&d, 0x02, 180000, 0) != 0) {
        fprintf(stderr, "échec de la vérification : firmware courant conservé\n");
        free(fw);
        close_device(&d);
        return RC_VERIFY;
    }

    printf("Finalisation…\n");
    memset(buf, 0, sizeof(buf));
    buf[0] = REPORT_FW;
    buf[1] = 0x03;
    send_cmd(&d, buf);

    free(fw);
    close_device(&d);

    printf("Mise à jour terminée. Débranchez puis rebranchez la manette.\n");
    return RC_OK;
}

static void usage(const char *prog)
{
    printf("Usage : %s info | flash FICHIER\n", prog);
    printf("\n");
    printf("  info          état de la manette (clé=valeur, pour script)\n");
    printf("  flash FICHIER écrit un firmware officiel Sony (950272 octets, USB)\n");
    printf("\n");
    printf("Codes de sortie : 0 ok, 1 usage, 3 manette absente, 4 Bluetooth,\n");
    printf("5 batterie faible, 6 image invalide, 7 erreur d'E/S, 8 image refusée,\n");
    printf("9 vérification échouée.\n");
}

int main(int argc, char **argv)
{
    if (argc < 2) {
        usage(argv[0]);
        return RC_USAGE;
    }
    if (!strcmp(argv[1], "info"))
        return cmd_info();
    if (!strcmp(argv[1], "flash")) {
        if (argc < 3) {
            usage(argv[0]);
            return RC_USAGE;
        }
        return cmd_flash(argv[2]);
    }
    if (!strcmp(argv[1], "-h") || !strcmp(argv[1], "--help")) {
        usage(argv[0]);
        return RC_OK;
    }
    usage(argv[0]);
    return RC_USAGE;
}
