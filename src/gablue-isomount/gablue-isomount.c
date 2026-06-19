/*
 * gablue-isomount - Monte un fichier image disque via UDisks2 et ouvre
 *                   une nouvelle fenetre Dolphin (contourne le bug KDE #471487
 *                   ou le panneau Devices ne se rafraichit pas apres montage)
 *
 * Comportement :
 *   - Si l'image est deja montee -> ouvre juste une nouvelle fenetre
 *   - Sinon -> monte via LoopSetup, ouvre Dolphin, surveille la fin de session
 *   - Demontage automatique quand toutes les fenetres Dolphin sont fermees
 *   - Si le device est occupe, attend sa liberation avant demontage
 *   - ISO9660 : no-part-scan pour eviter les fausses partitions (PTTYPE=mac)
 *   - Images disque (MBR/GPT) : part-scan + mount sur la partition enfant
 */

#define _GNU_SOURCE
#include <dbus/dbus.h>
#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <sys/wait.h>

#define TIMEOUT_MS 30000
#define MOUNT_RETRIES 15
#define MOUNT_RETRY_DELAY_US 200000

static void die(const char *msg) {
    fprintf(stderr, "%s\n", msg);
    exit(1);
}

static int is_iso9660(const char *path) {
    int fd = open(path, O_RDONLY);
    if (fd == -1) return 0;
    char buf[5];
    if (lseek(fd, 32769, SEEK_SET) == -1) { close(fd); return 0; }
    if (read(fd, buf, 5) != 5) { close(fd); return 0; }
    close(fd);
    return memcmp(buf, "CD001", 5) == 0;
}

static char *udisks_loop_setup(DBusConnection *sys, const char *iso_path, int no_part_scan) {
    int fd = open(iso_path, O_RDONLY);
    if (fd == -1) {
        fprintf(stderr, "Erreur ouverture %s: %s\n", iso_path, strerror(errno));
        return NULL;
    }

    DBusMessage *msg = dbus_message_new_method_call(
        "org.freedesktop.UDisks2",
        "/org/freedesktop/UDisks2/Manager",
        "org.freedesktop.UDisks2.Manager",
        "LoopSetup");

    DBusMessageIter args;
    dbus_message_iter_init_append(msg, &args);

    if (!dbus_message_iter_append_basic(&args, DBUS_TYPE_UNIX_FD, &fd)) {
        close(fd);
        dbus_message_unref(msg);
        die("Erreur: le bus systeme ne supporte pas le passage de fd");
    }

    DBusMessageIter dict, entry, variant;
    dbus_message_iter_open_container(&args, DBUS_TYPE_ARRAY, "{sv}", &dict);

    if (no_part_scan) {
        dbus_message_iter_open_container(&dict, DBUS_TYPE_DICT_ENTRY, NULL, &entry);
        const char *key = "no-part-scan";
        dbus_message_iter_append_basic(&entry, DBUS_TYPE_STRING, &key);
        dbus_message_iter_open_container(&entry, DBUS_TYPE_VARIANT, "b", &variant);
        dbus_bool_t val = TRUE;
        dbus_message_iter_append_basic(&variant, DBUS_TYPE_BOOLEAN, &val);
        dbus_message_iter_close_container(&entry, &variant);
        dbus_message_iter_close_container(&dict, &entry);
    }

    dbus_message_iter_close_container(&args, &dict);

    DBusError error;
    dbus_error_init(&error);
    DBusMessage *reply = dbus_connection_send_with_reply_and_block(
        sys, msg, TIMEOUT_MS, &error);
    dbus_message_unref(msg);
    close(fd);

    if (dbus_error_is_set(&error)) {
        fprintf(stderr, "LoopSetup echoue: %s\n", error.message);
        dbus_error_free(&error);
        return NULL;
    }

    DBusMessageIter reply_args;
    dbus_message_iter_init(reply, &reply_args);
    char *obj_path = NULL;
    dbus_message_iter_get_basic(&reply_args, &obj_path);
    char *result = strdup(obj_path);
    dbus_message_unref(reply);

    printf("Loop device cree: %s (no-part-scan=%d)\n", result, no_part_scan);
    return result;
}

static char *udisks_mount(DBusConnection *sys, const char *obj_path) {
    for (int attempt = 0; attempt < MOUNT_RETRIES; attempt++) {
        DBusMessage *msg = dbus_message_new_method_call(
            "org.freedesktop.UDisks2",
            obj_path,
            "org.freedesktop.UDisks2.Filesystem",
            "Mount");

        DBusMessageIter args;
        dbus_message_iter_init_append(msg, &args);

        DBusMessageIter dict;
        dbus_message_iter_open_container(&args, DBUS_TYPE_ARRAY, "{sv}", &dict);
        dbus_message_iter_close_container(&args, &dict);

        DBusError error;
        dbus_error_init(&error);
        DBusMessage *reply = dbus_connection_send_with_reply_and_block(
            sys, msg, TIMEOUT_MS, &error);
        dbus_message_unref(msg);

        if (reply) {
            DBusMessageIter reply_args;
            dbus_message_iter_init(reply, &reply_args);
            char *mount_point = NULL;
            dbus_message_iter_get_basic(&reply_args, &mount_point);
            char *result = strdup(mount_point);
            dbus_message_unref(reply);
            printf("Monte sur: %s\n", result);
            return result;
        }

        if (dbus_error_is_set(&error)) {
            if (strstr(error.message, "No such interface") && attempt < MOUNT_RETRIES - 1) {
                dbus_error_free(&error);
                usleep(MOUNT_RETRY_DELAY_US);
                continue;
            }
            fprintf(stderr, "Mount echoue sur %s: %s\n", obj_path, error.message);
            dbus_error_free(&error);
            return NULL;
        }
        return NULL;
    }
    fprintf(stderr, "Mount echoue: interface Filesystem non disponible apres 3s sur %s\n", obj_path);
    return NULL;
}

static void open_dolphin_window(const char *mount_point) {
    printf("Ouverture de Dolphin sur %s\n", mount_point);
    if (!fork()) {
        execlp("dolphin", "dolphin", mount_point, NULL);
        _exit(1);
    }
}

static int try_unmount(DBusConnection *sys, const char *obj_path) {
    DBusMessage *msg = dbus_message_new_method_call(
        "org.freedesktop.UDisks2",
        obj_path,
        "org.freedesktop.UDisks2.Filesystem",
        "Unmount");

    DBusMessageIter args;
    dbus_message_iter_init_append(msg, &args);
    DBusMessageIter dict;
    dbus_message_iter_open_container(&args, DBUS_TYPE_ARRAY, "{sv}", &dict);
    dbus_message_iter_close_container(&args, &dict);

    DBusError error;
    dbus_error_init(&error);
    DBusMessage *reply = dbus_connection_send_with_reply_and_block(
        sys, msg, 5000, &error);
    dbus_message_unref(msg);

    if (reply) {
        dbus_message_unref(reply);
        return 0;
    }

    if (dbus_error_is_set(&error)) {
        fprintf(stderr, "Unmount differe (device occupe): %s\n", error.message);
        dbus_error_free(&error);
    }
    return -1;
}

static void udisks_loop_delete(DBusConnection *sys, const char *obj_path) {
    DBusMessage *msg = dbus_message_new_method_call(
        "org.freedesktop.UDisks2",
        obj_path,
        "org.freedesktop.UDisks2.Loop",
        "Delete");

    DBusMessageIter args;
    dbus_message_iter_init_append(msg, &args);
    DBusMessageIter dict;
    dbus_message_iter_open_container(&args, DBUS_TYPE_ARRAY, "{sv}", &dict);
    dbus_message_iter_close_container(&args, &dict);

    DBusError error;
    dbus_error_init(&error);
    DBusMessage *reply = dbus_connection_send_with_reply_and_block(
        sys, msg, 5000, &error);
    if (reply) {
        printf("Loop device supprime: %s\n", obj_path);
        dbus_message_unref(reply);
    } else if (dbus_error_is_set(&error)) {
        fprintf(stderr, "LoopDelete echoue: %s\n", error.message);
        dbus_error_free(&error);
    }
    dbus_message_unref(msg);
}

static int count_dolphin_instances(DBusConnection *session) {
    DBusMessage *msg = dbus_message_new_method_call(
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
        "ListNames");

    DBusError error;
    dbus_error_init(&error);
    DBusMessage *reply = dbus_connection_send_with_reply_and_block(
        session, msg, 2000, &error);
    dbus_message_unref(msg);

    if (dbus_error_is_set(&error)) {
        dbus_error_free(&error);
        return -1;
    }

    int count = 0;
    DBusMessageIter reply_iter;
    dbus_message_iter_init(reply, &reply_iter);
    DBusMessageIter array_iter;
    dbus_message_iter_recurse(&reply_iter, &array_iter);

    while (dbus_message_iter_get_arg_type(&array_iter) != DBUS_TYPE_INVALID) {
        char *name;
        dbus_message_iter_get_basic(&array_iter, &name);
        if (strncmp(name, "org.kde.dolphin-", 16) == 0)
            count++;
        dbus_message_iter_next(&array_iter);
    }

    dbus_message_unref(reply);
    return count;
}

static char *find_existing_mount(const char *iso_path) {
    char cmd[4096];
    char loop[128] = {0};

    snprintf(cmd, sizeof(cmd),
        "losetup --associated '%s' --output NAME --noheadings 2>/dev/null", iso_path);
    FILE *f = popen(cmd, "r");
    if (!f) return NULL;
    if (!fgets(loop, sizeof(loop), f)) { pclose(f); return NULL; }
    pclose(f);
    loop[strcspn(loop, "\n")] = 0;
    if (loop[0] == '\0') return NULL;

    snprintf(cmd, sizeof(cmd),
        "findmnt -n -o TARGET --source '%s' 2>/dev/null", loop);
    f = popen(cmd, "r");
    if (!f) return NULL;
    char mount[4096] = {0};
    if (!fgets(mount, sizeof(mount), f)) { pclose(f); return NULL; }
    pclose(f);
    mount[strcspn(mount, "\n")] = 0;

    if (mount[0] == '\0') {
        snprintf(cmd, sizeof(cmd),
            "findmnt -n -o TARGET --source '%sp1' 2>/dev/null", loop);
        f = popen(cmd, "r");
        if (!f) return NULL;
        if (!fgets(mount, sizeof(mount), f)) { pclose(f); return NULL; }
        pclose(f);
        mount[strcspn(mount, "\n")] = 0;
    }

    if (mount[0] == '\0') return NULL;
    return strdup(mount);
}

int main(int argc, char **argv) {
    if (argc < 2) {
        fprintf(stderr, "Usage: %s <fichier_image>\n", argv[0]);
        return 1;
    }

    const char *iso_path = argv[1];

    FILE *log = fopen("/tmp/gablue-isomount.log", "a");
    if (log) {
        fprintf(log, "\n=== %s ===\n", iso_path);
        fflush(log);
        dup2(fileno(log), STDOUT_FILENO);
        dup2(fileno(log), STDERR_FILENO);
        setvbuf(stdout, NULL, _IONBF, 0);
        setvbuf(stderr, NULL, _IONBF, 0);
        fclose(log);
    }

    char *existing = find_existing_mount(iso_path);
    if (existing) {
        printf("Image deja montee sur %s, ouverture de Dolphin\n", existing);
        open_dolphin_window(existing);
        free(existing);
        return 0;
    }

    DBusConnection *sys = dbus_bus_get(DBUS_BUS_SYSTEM, NULL);
    if (!sys)
        die("Erreur connexion bus systeme");

    DBusConnection *session = dbus_bus_get(DBUS_BUS_SESSION, NULL);
    if (!session) {
        dbus_connection_unref(sys);
        die("Erreur connexion bus session");
    }

    int iso = is_iso9660(iso_path);
    printf("Type detecte: %s\n", iso ? "ISO9660" : "image disque");

    char *loop_path = udisks_loop_setup(sys, iso_path, iso);
    if (!loop_path) {
        dbus_connection_unref(session);
        dbus_connection_unref(sys);
        return 1;
    }

    char *mount_path = loop_path;
    char part_path[512];
    char *mount_point = NULL;

    if (iso) {
        mount_point = udisks_mount(sys, loop_path);
    } else {
        snprintf(part_path, sizeof(part_path), "%sp1", loop_path);
        printf("Tentative mount partition %s\n", part_path);
        mount_point = udisks_mount(sys, part_path);
        if (mount_point) {
            mount_path = part_path;
        } else {
            printf("Partition non trouvee, tentative sur device parent\n");
            mount_point = udisks_mount(sys, loop_path);
        }
    }

    if (!mount_point) {
        udisks_loop_delete(sys, loop_path);
        free(loop_path);
        dbus_connection_unref(session);
        dbus_connection_unref(sys);
        return 1;
    }

    open_dolphin_window(mount_point);

    printf("Surveillance des instances Dolphin...\n");
    while (1) {
        sleep(1);
        int n = count_dolphin_instances(session);
        if (n == 0) {
            sleep(1);
            if (count_dolphin_instances(session) == 0) {
                if (try_unmount(sys, mount_path) == 0)
                    break;
            }
        }
    }

    udisks_loop_delete(sys, loop_path);

    free(mount_point);
    free(loop_path);
    dbus_connection_unref(session);
    dbus_connection_unref(sys);

    return 0;
}
