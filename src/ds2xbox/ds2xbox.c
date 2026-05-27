#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <fcntl.h>
#include <errno.h>
#include <signal.h>
#include <pthread.h>
#include <linux/input.h>
#include <linux/uinput.h>
#include <sys/epoll.h>
#include <sys/ioctl.h>
#include <sys/poll.h>
#include <dirent.h>
#include <stdbool.h>
#include <time.h>
#include <getopt.h>

#define MAX_CONTROLLERS 4

enum controller_filter { FILTER_ALL, FILTER_DUALSENSE, FILTER_DS4 };
enum bus_filter { BUS_FILTER_ALL, BUS_FILTER_USB, BUS_FILTER_BT };

static enum controller_filter filter = FILTER_ALL;
static enum bus_filter busfilter = BUS_FILTER_ALL;
#define VID_SONY 0x054c
#define VID_XBOX 0x045e
#define PID_XBOX360 0x028e
#define FF_MIN_INTERVAL_MS 16

struct controller_map {
    uint16_t pid;
    const char *name;
};

static const struct controller_map sony_controllers[] = {
    { 0x0ce6, "DualSense" },
    { 0x0df2, "DualSense Edge" },
    { 0x05c4, "DualShock 4" },
    { 0x09cc, "DualShock 4 Slim" },
    { 0x0ba0, "DualShock 4 Adapter" },
    { 0, NULL }
};

struct controller {
    int fd_src;
    int fd_dst;
    char dev_path[256];
    char name[256];
    char xbox_path[256];
    uint16_t vid;
    uint16_t pid;
    uint16_t bustype;
    pthread_t ff_thread;
    bool ff_running;
    int sony_ff_id;
    bool sony_ff_uploaded;
    uint16_t current_strong;
    uint16_t current_weak;
    bool is_playing;
    struct timespec last_ff_event;
};

static volatile bool running = true;

static bool matches_filter(uint16_t pid) {
    if (filter == FILTER_ALL) return true;
    if (filter == FILTER_DUALSENSE)
        return pid == 0x0ce6 || pid == 0x0df2;
    if (filter == FILTER_DS4)
        return pid == 0x05c4 || pid == 0x09cc || pid == 0x0ba0;
    return true;
}

static bool matches_bus_filter(uint16_t bustype) {
    if (busfilter == BUS_FILTER_ALL) return true;
    if (busfilter == BUS_FILTER_USB)
        return bustype == BUS_USB;
    if (busfilter == BUS_FILTER_BT)
        return bustype == BUS_BLUETOOTH;
    return true;
}

static void print_usage(const char *prog) {
    printf("Usage: %s [OPTIONS]\n\n", prog);
    printf("Options:\n");
    printf("  --dualsense    Emulate only DualSense controllers\n");
    printf("  --ds4          Emulate only DualShock 4 controllers\n");
    printf("  --usb          Emulate only USB-connected controllers\n");
    printf("  --bt           Emulate only Bluetooth-connected controllers\n");
    printf("  -h, --help     Show this help\n");
}

static void signal_handler(int sig) {
    (void)sig;
    running = false;
}

static long time_diff_ms(struct timespec *a, struct timespec *b) {
    return (a->tv_sec - b->tv_sec) * 1000 + (a->tv_nsec - b->tv_nsec) / 1000000;
}

static void stop_rumble(struct controller *ctrl) {
    if (ctrl->sony_ff_uploaded && ctrl->is_playing) {
        struct input_event ev = {
            .type = EV_FF,
            .code = ctrl->sony_ff_id,
            .value = 0
        };
        write(ctrl->fd_src, &ev, sizeof(ev));
        ctrl->is_playing = false;
    }
}

static void play_rumble(struct controller *ctrl) {
    if (!ctrl->sony_ff_uploaded)
        return;
    
    struct input_event ev = {
        .type = EV_FF,
        .code = ctrl->sony_ff_id,
        .value = 1
    };
    write(ctrl->fd_src, &ev, sizeof(ev));
    ctrl->is_playing = true;
}

static void update_effect(struct controller *ctrl, uint16_t strong, uint16_t weak) {
    if (strong == 0 && weak == 0) {
        stop_rumble(ctrl);
        ctrl->current_strong = 0;
        ctrl->current_weak = 0;
        return;
    }
    
    bool need_upload = !ctrl->sony_ff_uploaded;
    bool need_update = (strong != ctrl->current_strong || weak != ctrl->current_weak);
    
    if (need_upload || need_update) {
        struct ff_effect effect;
        memset(&effect, 0, sizeof(effect));
        effect.type = FF_RUMBLE;
        effect.id = ctrl->sony_ff_uploaded ? ctrl->sony_ff_id : -1;
        effect.replay.length = 100;
        effect.u.rumble.strong_magnitude = strong;
        effect.u.rumble.weak_magnitude = weak;
        
        if (ioctl(ctrl->fd_src, EVIOCSFF, &effect) < 0) {
            ctrl->sony_ff_uploaded = false;
            return;
        }
        ctrl->sony_ff_id = effect.id;
        ctrl->sony_ff_uploaded = true;
        ctrl->is_playing = false;
    }
    
    ctrl->current_strong = strong;
    ctrl->current_weak = weak;
}

static void handle_ff_upload(struct controller *ctrl, uint32_t request_id) {
    struct uinput_ff_upload upload;
    memset(&upload, 0, sizeof(upload));
    upload.request_id = request_id;
    
    if (ioctl(ctrl->fd_dst, UI_BEGIN_FF_UPLOAD, &upload) < 0) {
        perror("UI_BEGIN_FF_UPLOAD");
        return;
    }
    
    if (upload.effect.type == FF_RUMBLE) {
        update_effect(ctrl,
                      upload.effect.u.rumble.strong_magnitude,
                      upload.effect.u.rumble.weak_magnitude);
    }
    
    upload.retval = 0;
    
    if (ioctl(ctrl->fd_dst, UI_END_FF_UPLOAD, &upload) < 0) {
        perror("UI_END_FF_UPLOAD");
    }
}

static void handle_ff_erase(struct controller *ctrl, uint32_t request_id) {
    struct uinput_ff_erase erase;
    memset(&erase, 0, sizeof(erase));
    erase.request_id = request_id;
    
    if (ioctl(ctrl->fd_dst, UI_BEGIN_FF_ERASE, &erase) < 0) {
        perror("UI_BEGIN_FF_ERASE");
        return;
    }
    
    stop_rumble(ctrl);
    
    erase.retval = 0;
    
    if (ioctl(ctrl->fd_dst, UI_END_FF_ERASE, &erase) < 0) {
        perror("UI_END_FF_ERASE");
    }
}

static void handle_ff_play(struct controller *ctrl, int value) {
    if (value == 0) {
        stop_rumble(ctrl);
        return;
    }
    
    if (ctrl->current_strong == 0 && ctrl->current_weak == 0) {
        return;
    }
    
    struct timespec now;
    clock_gettime(CLOCK_MONOTONIC, &now);
    
    if (time_diff_ms(&now, &ctrl->last_ff_event) < FF_MIN_INTERVAL_MS) {
        return;
    }
    ctrl->last_ff_event = now;
    
    play_rumble(ctrl);
}

static void *ff_thread_func(void *arg) {
    struct controller *ctrl = (struct controller *)arg;
    struct input_event ev;
    struct pollfd pfd;
    
    pfd.fd = ctrl->fd_dst;
    pfd.events = POLLIN;
    
    while (ctrl->ff_running && running) {
        int ret = poll(&pfd, 1, 5);
        if (ret < 0) {
            if (errno == EINTR)
                continue;
            break;
        }
        
        if (ret == 0)
            continue;
        
        while (1) {
            ssize_t bytes = read(ctrl->fd_dst, &ev, sizeof(ev));
            if (bytes < 0) {
                if (errno == EAGAIN || errno == EWOULDBLOCK)
                    break;
                break;
            }
            
            if (bytes == sizeof(ev)) {
                if (ev.type == EV_UINPUT) {
                    if (ev.code == UI_FF_UPLOAD) {
                        handle_ff_upload(ctrl, ev.value);
                    } else if (ev.code == UI_FF_ERASE) {
                        handle_ff_erase(ctrl, ev.value);
                    }
                } else if (ev.type == EV_FF) {
                    handle_ff_play(ctrl, ev.value);
                }
            }
        }
    }
    
    return NULL;
}

static int create_xbox_device(struct controller *ctrl) {
    struct uinput_setup usetup;
    struct uinput_abs_setup abs_setup;
    char sysname[64];
    
    ctrl->fd_dst = open("/dev/uinput", O_RDWR | O_NONBLOCK);
    if (ctrl->fd_dst < 0) {
        perror("open /dev/uinput");
        return -1;
    }
    
    ioctl(ctrl->fd_dst, UI_SET_EVBIT, EV_KEY);
    ioctl(ctrl->fd_dst, UI_SET_EVBIT, EV_ABS);
    ioctl(ctrl->fd_dst, UI_SET_EVBIT, EV_SYN);
    ioctl(ctrl->fd_dst, UI_SET_EVBIT, EV_FF);
    
    ioctl(ctrl->fd_dst, UI_SET_KEYBIT, BTN_A);
    ioctl(ctrl->fd_dst, UI_SET_KEYBIT, BTN_B);
    ioctl(ctrl->fd_dst, UI_SET_KEYBIT, BTN_X);
    ioctl(ctrl->fd_dst, UI_SET_KEYBIT, BTN_Y);
    ioctl(ctrl->fd_dst, UI_SET_KEYBIT, BTN_TL);
    ioctl(ctrl->fd_dst, UI_SET_KEYBIT, BTN_TR);
    ioctl(ctrl->fd_dst, UI_SET_KEYBIT, BTN_SELECT);
    ioctl(ctrl->fd_dst, UI_SET_KEYBIT, BTN_START);
    ioctl(ctrl->fd_dst, UI_SET_KEYBIT, BTN_MODE);
    ioctl(ctrl->fd_dst, UI_SET_KEYBIT, BTN_THUMBL);
    ioctl(ctrl->fd_dst, UI_SET_KEYBIT, BTN_THUMBR);
    
    ioctl(ctrl->fd_dst, UI_SET_ABSBIT, ABS_X);
    ioctl(ctrl->fd_dst, UI_SET_ABSBIT, ABS_Y);
    ioctl(ctrl->fd_dst, UI_SET_ABSBIT, ABS_RX);
    ioctl(ctrl->fd_dst, UI_SET_ABSBIT, ABS_RY);
    ioctl(ctrl->fd_dst, UI_SET_ABSBIT, ABS_Z);
    ioctl(ctrl->fd_dst, UI_SET_ABSBIT, ABS_RZ);
    ioctl(ctrl->fd_dst, UI_SET_ABSBIT, ABS_HAT0X);
    ioctl(ctrl->fd_dst, UI_SET_ABSBIT, ABS_HAT0Y);
    
    ioctl(ctrl->fd_dst, UI_SET_FFBIT, FF_RUMBLE);
    
    memset(&abs_setup, 0, sizeof(abs_setup));
    abs_setup.code = ABS_X;
    abs_setup.absinfo.minimum = -32768;
    abs_setup.absinfo.maximum = 32767;
    abs_setup.absinfo.fuzz = 16;
    abs_setup.absinfo.flat = 128;
    ioctl(ctrl->fd_dst, UI_ABS_SETUP, &abs_setup);
    
    abs_setup.code = ABS_Y;
    ioctl(ctrl->fd_dst, UI_ABS_SETUP, &abs_setup);
    
    abs_setup.code = ABS_RX;
    ioctl(ctrl->fd_dst, UI_ABS_SETUP, &abs_setup);
    
    abs_setup.code = ABS_RY;
    ioctl(ctrl->fd_dst, UI_ABS_SETUP, &abs_setup);
    
    abs_setup.code = ABS_Z;
    abs_setup.absinfo.minimum = 0;
    abs_setup.absinfo.maximum = 255;
    abs_setup.absinfo.fuzz = 0;
    abs_setup.absinfo.flat = 0;
    ioctl(ctrl->fd_dst, UI_ABS_SETUP, &abs_setup);
    
    abs_setup.code = ABS_RZ;
    ioctl(ctrl->fd_dst, UI_ABS_SETUP, &abs_setup);
    
    abs_setup.code = ABS_HAT0X;
    abs_setup.absinfo.minimum = -1;
    abs_setup.absinfo.maximum = 1;
    ioctl(ctrl->fd_dst, UI_ABS_SETUP, &abs_setup);
    
    abs_setup.code = ABS_HAT0Y;
    ioctl(ctrl->fd_dst, UI_ABS_SETUP, &abs_setup);
    
    memset(&usetup, 0, sizeof(usetup));
    usetup.id.bustype = BUS_USB;
    usetup.id.vendor = VID_XBOX;
    usetup.id.product = PID_XBOX360;
    usetup.ff_effects_max = 16;
    strcpy(usetup.name, "Xbox 360 Controller");
    
    if (ioctl(ctrl->fd_dst, UI_DEV_SETUP, &usetup) < 0) {
        perror("UI_DEV_SETUP");
        close(ctrl->fd_dst);
        return -1;
    }
    
    if (ioctl(ctrl->fd_dst, UI_DEV_CREATE) < 0) {
        perror("UI_DEV_CREATE");
        close(ctrl->fd_dst);
        return -1;
    }
    
    if (ioctl(ctrl->fd_dst, UI_GET_SYSNAME(64), sysname) < 0) {
        perror("UI_GET_SYSNAME");
    } else {
        snprintf(ctrl->xbox_path, sizeof(ctrl->xbox_path), "/dev/input/%s", sysname);
    }
    
    return 0;
}

static const char *get_controller_name(uint16_t pid) {
    for (int i = 0; sony_controllers[i].name; i++) {
        if (sony_controllers[i].pid == pid)
            return sony_controllers[i].name;
    }
    return "Unknown Sony Controller";
}

static bool is_sony_controller(uint16_t vid, uint16_t pid) {
    if (vid != VID_SONY)
        return false;
    for (int i = 0; sony_controllers[i].name; i++) {
        if (sony_controllers[i].pid == pid)
            return true;
    }
    return false;
}

static int find_controllers(struct controller *controllers, int max) {
    DIR *dir;
    struct dirent *ent;
    int count = 0;
    char path[512];
    
    dir = opendir("/dev/input");
    if (!dir) {
        perror("opendir /dev/input");
        return 0;
    }
    
    while ((ent = readdir(dir)) != NULL && count < max) {
        if (strncmp(ent->d_name, "event", 5) != 0)
            continue;
        
        snprintf(path, sizeof(path), "/dev/input/%s", ent->d_name);
        
        int fd = open(path, O_RDWR | O_NONBLOCK);
        if (fd < 0)
            continue;
        
        struct input_id id;
        if (ioctl(fd, EVIOCGID, &id) < 0) {
            close(fd);
            continue;
        }
        
        char name[256] = {0};
        ioctl(fd, EVIOCGNAME(sizeof(name)), name);
        
        if (is_sony_controller(id.vendor, id.product) && matches_filter(id.product) && matches_bus_filter(id.bustype)) {
            bool is_main = strstr(name, "Touchpad") == NULL && 
                          strstr(name, "Motion") == NULL &&
                          strstr(name, "Headset") == NULL &&
                          strstr(name, "Jack") == NULL;
            
            if (is_main) {
                controllers[count].fd_src = fd;
                controllers[count].vid = id.vendor;
                controllers[count].pid = id.product;
                controllers[count].bustype = id.bustype;
                controllers[count].fd_dst = -1;
                controllers[count].ff_running = false;
                controllers[count].sony_ff_id = -1;
                controllers[count].sony_ff_uploaded = false;
                controllers[count].current_strong = 0;
                controllers[count].current_weak = 0;
                controllers[count].is_playing = false;
                memset(&controllers[count].last_ff_event, 0, sizeof(struct timespec));
                snprintf(controllers[count].dev_path, sizeof(controllers[count].dev_path), "%s", path);
                snprintf(controllers[count].name, sizeof(controllers[count].name), "%s", name);
                
                printf("Found: %s (%04x:%04x) on %s [%s]\n", 
                       get_controller_name(id.product),
                       id.vendor, id.product, path,
                       id.bustype == BUS_BLUETOOTH ? "BT" : "USB");
                count++;
            } else {
                close(fd);
            }
        } else {
            close(fd);
        }
    }
    
    closedir(dir);
    return count;
}

static int convert_stick_value(int value) {
    int converted = (value - 128) * 256;
    if (converted > 32767) converted = 32767;
    if (converted < -32768) converted = -32768;
    return converted;
}

static void map_ps_to_xbox(struct input_event *ev) {
    switch (ev->code) {
    case BTN_SOUTH:   ev->code = BTN_A; break;
    case BTN_EAST:    ev->code = BTN_B; break;
    case BTN_WEST:    ev->code = BTN_X; break;
    case BTN_NORTH:   ev->code = BTN_Y; break;
    case BTN_TL:      ev->code = BTN_TL; break;
    case BTN_TR:      ev->code = BTN_TR; break;
    case BTN_TL2:     break;
    case BTN_TR2:     break;
    case BTN_SELECT:  ev->code = BTN_SELECT; break;
    case BTN_START:   ev->code = BTN_START; break;
    case BTN_MODE:    ev->code = BTN_MODE; break;
    case BTN_THUMBL:  ev->code = BTN_THUMBL; break;
    case BTN_THUMBR:  ev->code = BTN_THUMBR; break;
    }
}

static void map_axis_to_xbox(struct input_event *ev) {
    switch (ev->code) {
    case ABS_X:
    case ABS_Y:
    case ABS_RX:
    case ABS_RY:
        ev->value = convert_stick_value(ev->value);
        break;
    case ABS_Z:
    case ABS_RZ:
    case ABS_HAT0X:
    case ABS_HAT0Y:
        break;
    default:
        break;
    }
}

static void handle_event(struct controller *ctrl) {
    struct input_event ev;
    ssize_t bytes;
    
    while ((bytes = read(ctrl->fd_src, &ev, sizeof(ev))) > 0) {
        if (bytes != sizeof(ev))
            continue;
        
        if (ev.type == EV_KEY) {
            map_ps_to_xbox(&ev);
        } else if (ev.type == EV_ABS) {
            map_axis_to_xbox(&ev);
        }
        
        if (write(ctrl->fd_dst, &ev, sizeof(ev)) < 0) {
            if (errno != EAGAIN && errno != EWOULDBLOCK)
                perror("write uinput");
        }
    }
    
    if (bytes == 0 || (bytes < 0 && errno != EAGAIN && errno != EWOULDBLOCK)) {
        ctrl->ff_running = false;
        if (ctrl->ff_thread) {
            pthread_join(ctrl->ff_thread, NULL);
            ctrl->ff_thread = 0;
        }
        if (ctrl->sony_ff_uploaded) {
            ioctl(ctrl->fd_src, EVIOCRMFF, ctrl->sony_ff_id);
            ctrl->sony_ff_uploaded = false;
        }
        if (ctrl->fd_dst >= 0) {
            ioctl(ctrl->fd_dst, UI_DEV_DESTROY);
            close(ctrl->fd_dst);
            ctrl->fd_dst = -1;
        }
        if (ctrl->fd_src >= 0) {
            close(ctrl->fd_src);
            ctrl->fd_src = -1;
        }
        ctrl->dev_path[0] = '\0';
        printf("[HOTPLUG] Disconnected: %s\n", ctrl->name);
    }
}

static void cleanup(struct controller *controllers, int count) {
    for (int i = 0; i < count; i++) {
        controllers[i].ff_running = false;
        if (controllers[i].ff_thread) {
            pthread_join(controllers[i].ff_thread, NULL);
        }
        if (controllers[i].sony_ff_uploaded && controllers[i].fd_src >= 0) {
            stop_rumble(&controllers[i]);
            ioctl(controllers[i].fd_src, EVIOCRMFF, controllers[i].sony_ff_id);
        }
        if (controllers[i].fd_dst >= 0) {
            ioctl(controllers[i].fd_dst, UI_DEV_DESTROY);
            close(controllers[i].fd_dst);
        }
        if (controllers[i].fd_src >= 0) {
            close(controllers[i].fd_src);
        }
    }
}

static int start_ff_thread(struct controller *ctrl) {
    ctrl->ff_running = true;
    if (pthread_create(&ctrl->ff_thread, NULL, ff_thread_func, ctrl) != 0) {
        perror("pthread_create FF");
        ctrl->ff_running = false;
        return -1;
    }
    return 0;
}

static bool is_controller_connected(struct controller *controllers, int count, const char *path) {
    for (int i = 0; i < count; i++) {
        if (controllers[i].fd_src >= 0 && strcmp(controllers[i].dev_path, path) == 0)
            return true;
    }
    return false;
}

static int scan_for_new_controllers(struct controller *controllers, int *count, int epoll_fd) {
    DIR *dir;
    struct dirent *ent;
    int added = 0;
    char path[512];
    
    dir = opendir("/dev/input");
    if (!dir)
        return 0;
    
    while ((ent = readdir(dir)) != NULL) {
        if (strncmp(ent->d_name, "event", 5) != 0)
            continue;
        
        snprintf(path, sizeof(path), "/dev/input/%s", ent->d_name);
        
        if (is_controller_connected(controllers, *count, path))
            continue;
        
        int fd = open(path, O_RDWR | O_NONBLOCK);
        if (fd < 0)
            continue;
        
        struct input_id id;
        if (ioctl(fd, EVIOCGID, &id) < 0) {
            close(fd);
            continue;
        }
        
        char name[256] = {0};
        ioctl(fd, EVIOCGNAME(sizeof(name)), name);
        
        if (is_sony_controller(id.vendor, id.product) && matches_filter(id.product) && matches_bus_filter(id.bustype)) {
            bool is_main = strstr(name, "Touchpad") == NULL && 
                          strstr(name, "Motion") == NULL &&
                          strstr(name, "Headset") == NULL &&
                          strstr(name, "Jack") == NULL;
            
            if (is_main) {
                int slot = -1;
                for (int i = 0; i < *count; i++) {
                    if (controllers[i].fd_src < 0) {
                        slot = i;
                        break;
                    }
                }
                if (slot < 0 && *count < MAX_CONTROLLERS) {
                    slot = *count;
                    (*count)++;
                }
                if (slot < 0) {
                    close(fd);
                    continue;
                }
                
                struct controller *ctrl = &controllers[slot];
                ctrl->fd_src = fd;
                ctrl->vid = id.vendor;
                ctrl->pid = id.product;
                ctrl->bustype = id.bustype;
                ctrl->fd_dst = -1;
                ctrl->ff_running = false;
                ctrl->sony_ff_id = -1;
                ctrl->sony_ff_uploaded = false;
                ctrl->current_strong = 0;
                ctrl->current_weak = 0;
                ctrl->is_playing = false;
                memset(&ctrl->last_ff_event, 0, sizeof(ctrl->last_ff_event));
                snprintf(ctrl->dev_path, sizeof(ctrl->dev_path), "%s", path);
                snprintf(ctrl->name, sizeof(ctrl->name), "%s", name);
                
                if (create_xbox_device(ctrl) < 0) {
                    close(fd);
                    ctrl->fd_src = -1;
                    continue;
                }
                
                start_ff_thread(ctrl);
                
                struct epoll_event ev;
                ev.events = EPOLLIN | EPOLLET;
                ev.data.u32 = slot;
                epoll_ctl(epoll_fd, EPOLL_CTL_ADD, ctrl->fd_src, &ev);
                
                printf("[HOTPLUG] Connected: %s (%04x:%04x) [%s] -> %s\n",
                       get_controller_name(id.product), id.vendor, id.product,
                       id.bustype == BUS_BLUETOOTH ? "BT" : "USB", ctrl->xbox_path);
                
                added++;
            } else {
                close(fd);
            }
        } else {
            close(fd);
        }
    }
    
    closedir(dir);
    return added;
}

int main(int argc, char *argv[]) {
    static struct option long_options[] = {
        {"dualsense", no_argument, NULL, 'd'},
        {"ds4",       no_argument, NULL, 's'},
        {"usb",       no_argument, NULL, 'u'},
        {"bt",        no_argument, NULL, 'b'},
        {"help",      no_argument, NULL, 'h'},
        {0, 0, 0, 0}
    };
    
    int opt;
    while ((opt = getopt_long(argc, argv, "h", long_options, NULL)) != -1) {
        switch (opt) {
        case 'd':
            if (filter == FILTER_DS4) {
                fprintf(stderr, "Error: --dualsense and --ds4 are mutually exclusive\n");
                return 1;
            }
            filter = FILTER_DUALSENSE;
            break;
        case 's':
            if (filter == FILTER_DUALSENSE) {
                fprintf(stderr, "Error: --dualsense and --ds4 are mutually exclusive\n");
                return 1;
            }
            filter = FILTER_DS4;
            break;
        case 'u':
            if (busfilter == BUS_FILTER_BT) {
                fprintf(stderr, "Error: --usb and --bt are mutually exclusive\n");
                return 1;
            }
            busfilter = BUS_FILTER_USB;
            break;
        case 'b':
            if (busfilter == BUS_FILTER_USB) {
                fprintf(stderr, "Error: --usb and --bt are mutually exclusive\n");
                return 1;
            }
            busfilter = BUS_FILTER_BT;
            break;
        case 'h':
            print_usage(argv[0]);
            return 0;
        default:
            print_usage(argv[0]);
            return 1;
        }
    }
    
    struct controller controllers[MAX_CONTROLLERS];
    int count = 0;
    
    signal(SIGINT, signal_handler);
    signal(SIGTERM, signal_handler);
    
    printf("=== ds2xbox - Sony Controller to Xbox Emulator ===\n");
    if (filter == FILTER_DUALSENSE)
        printf("Mode: DualSense only\n");
    else if (filter == FILTER_DS4)
        printf("Mode: DualShock 4 only\n");
    if (busfilter == BUS_FILTER_USB)
        printf("Bus: USB only\n");
    else if (busfilter == BUS_FILTER_BT)
        printf("Bus: Bluetooth only\n");
    printf("\n");
    
    count = find_controllers(controllers, MAX_CONTROLLERS);
    
    int epoll_fd = epoll_create1(0);
    if (epoll_fd < 0) {
        perror("epoll_create1");
        cleanup(controllers, count);
        return 1;
    }
    
    if (count > 0) {
        printf("\nCreating %d virtual Xbox controller(s)...\n", count);
        
        for (int i = 0; i < count; i++) {
            if (create_xbox_device(&controllers[i]) < 0) {
                fprintf(stderr, "Failed to create Xbox device for %s\n", controllers[i].dev_path);
                continue;
            }
            printf("  [%d] Xbox controller created: %s\n", i + 1, controllers[i].xbox_path);
            
            if (start_ff_thread(&controllers[i]) < 0) {
                printf("  [%d] Warning: Force feedback not available\n", i + 1);
            } else {
                printf("  [%d] Force feedback enabled\n", i + 1);
            }
            
            struct epoll_event ev;
            ev.events = EPOLLIN | EPOLLET;
            ev.data.u32 = i;
            epoll_ctl(epoll_fd, EPOLL_CTL_ADD, controllers[i].fd_src, &ev);
        }
    } else {
        printf("No Sony controllers found. Waiting for connection...\n");
    }
    
    printf("\nMapping active - Press Ctrl+C to exit\n");
    printf("================================================\n\n");
    
    struct epoll_event events[MAX_CONTROLLERS];
    time_t last_scan = 0;
    
    while (running) {
        int nfds = epoll_wait(epoll_fd, events, MAX_CONTROLLERS, 1000);
        if (nfds < 0) {
            if (errno == EINTR)
                continue;
            perror("epoll_wait");
            break;
        }
        
        for (int i = 0; i < nfds; i++) {
            int idx = events[i].data.u32;
            if (idx >= 0 && idx < count && controllers[idx].fd_src >= 0) {
                handle_event(&controllers[idx]);
            }
        }
        
        time_t now = time(NULL);
        if (now - last_scan >= 2) {
            last_scan = now;
            scan_for_new_controllers(controllers, &count, epoll_fd);
        }
    }
    
    printf("\nShutting down...\n");
    close(epoll_fd);
    cleanup(controllers, count);
    printf("Done.\n");
    
    return 0;
}
