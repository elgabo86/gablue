// pad-inhibit - empêche la mise en veille / le verrouillage de l'écran tant
// qu'une manette est connectée.
//
// Même mécanisme que l'ancien gamepadshortcuts : org.freedesktop.ScreenSaver
// Inhibit sur la session. Une inhibition est liée à la connexion D-Bus qui l'a
// demandée : il FAUT garder la connexion ouverte (un `gdbus call` ponctuel
// rendrait le cookie mort à la sortie du processus).
//
// Cycle de vie : on inhibe à la connexion, on imprime le cookie, puis on
// attend la fermeture de stdin. EOF (le lanceur ferme le tube) ou SIGTERM/SIGINT
// -> UnInhibit + sortie. C'est gpad-trigger qui ouvre/ferme le tube selon qu'il y a
// une manette ou non.
#include <dbus/dbus.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

static DBusConnection *conn;
static dbus_uint32_t cookie = 0;
static volatile sig_atomic_t stop = 0;

static void on_sig(int s)
{
    (void)s;
    stop = 1;
}

/// Cookie d'inhibition, 0 si l'interface n'est pas disponible.
static dbus_uint32_t inhibit(void)
{
    DBusMessage *msg = dbus_message_new_method_call(
        "org.freedesktop.ScreenSaver", "/ScreenSaver",
        "org.freedesktop.ScreenSaver", "Inhibit");
    if (!msg)
        return 0;

    const char *app = "gablue-manette";
    const char *reason = "Manette connectee";
    dbus_message_append_args(msg, DBUS_TYPE_STRING, &app, DBUS_TYPE_STRING, &reason,
                             DBUS_TYPE_INVALID);

    DBusError err;
    dbus_error_init(&err);
    DBusMessage *reply =
        dbus_connection_send_with_reply_and_block(conn, msg, 2000, &err);
    dbus_message_unref(msg);

    dbus_uint32_t c = 0;
    if (dbus_error_is_set(&err) || !reply) {
        fprintf(stderr, "[inhibit] indisponible: %s\n",
                dbus_error_is_set(&err) ? err.message : "pas de reponse");
        if (dbus_error_is_set(&err))
            dbus_error_free(&err);
        if (reply)
            dbus_message_unref(reply);
        return 0;
    }
    if (!dbus_message_get_args(reply, &err, DBUS_TYPE_UINT32, &c, DBUS_TYPE_INVALID)) {
        fprintf(stderr, "[inhibit] cookie absent: %s\n", err.message);
        dbus_error_free(&err);
        dbus_message_unref(reply);
        return 0;
    }
    dbus_message_unref(reply);
    return c;
}

static void uninhibit(void)
{
    if (!conn || cookie == 0)
        return;
    DBusMessage *msg = dbus_message_new_method_call(
        "org.freedesktop.ScreenSaver", "/ScreenSaver",
        "org.freedesktop.ScreenSaver", "UnInhibit");
    if (!msg)
        return;
    dbus_message_append_args(msg, DBUS_TYPE_UINT32, &cookie, DBUS_TYPE_INVALID);
    dbus_connection_send_with_reply_and_block(conn, msg, 2000, NULL);
    dbus_message_unref(msg);
    printf("UNINHIBIT %u\n", cookie);
    fflush(stdout);
    cookie = 0;
}

int main(void)
{
    signal(SIGTERM, on_sig);
    signal(SIGINT, on_sig);

    DBusError err;
    dbus_error_init(&err);
    // private : la connexion appartient au processus, elle disparaît avec lui
    conn = dbus_bus_get_private(DBUS_BUS_SESSION, &err);
    if (dbus_error_is_set(&err) || !conn) {
        fprintf(stderr, "[inhibit] session D-Bus injoignable: %s\n",
                dbus_error_is_set(&err) ? err.message : "?");
        return 1;
    }
    dbus_connection_set_exit_on_disconnect(conn, FALSE);

    cookie = inhibit();
    if (cookie == 0)
        return 1;
    printf("INHIBIT %u\n", cookie);
    fflush(stdout);

    // tant que le tube reste ouvert, on tient l'inhibition
    char buf[64];
    while (!stop && read(STDIN_FILENO, buf, sizeof buf) > 0) {
    }

    uninhibit();
    dbus_connection_close(conn);
    dbus_connection_unref(conn);
    return 0;
}
