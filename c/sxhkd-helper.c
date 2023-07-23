#include "sxhkd-helper.h"
#include "../sxhkd/src/grab.h"
#include "../sxhkd/src/parse.h"
#include <fcntl.h>
#include <signal.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/select.h>
#include <sys/stat.h>
#include <sys/time.h>
#include <sys/types.h>
#include <unistd.h>
#include <xcb/xcb_event.h>

xcb_connection_t *dpy;
xcb_window_t root;
xcb_key_symbols_t *symbols;

char *shell;
char config_file[MAXLEN];
char *config_path;
char **extra_confs;
int num_extra_confs;
int redir_fd;
FILE *status_fifo;
char progress[3 * MAXLEN];
int mapping_count;
int timeout;

char sxhkd_pid[MAXLEN];

hotkey_t *hotkeys_head, *hotkeys_tail;
bool running, grabbed, toggle_grab, reload, bell, chained, locked;
bool initialized = false;
xcb_keysym_t abort_keysym;
chord_t *abort_chord;

uint16_t num_lock;
uint16_t caps_lock;
uint16_t scroll_lock;

void setup(void) {
  int screen_idx;
  dpy = xcb_connect(NULL, &screen_idx);
  if (xcb_connection_has_error(dpy))
    err("Can't open display.\n");
  xcb_screen_t *screen = NULL;
  xcb_screen_iterator_t screen_iter =
      xcb_setup_roots_iterator(xcb_get_setup(dpy));
  for (; screen_iter.rem; xcb_screen_next(&screen_iter), screen_idx--) {
    if (screen_idx == 0) {
      screen = screen_iter.data;
      break;
    }
  }
  if (screen == NULL)
    err("Can't acquire screen.\n");
  root = screen->root;
  if ((shell = getenv(SXHKD_SHELL_ENV)) == NULL &&
      (shell = getenv(SHELL_ENV)) == NULL)
    err("The '%s' environment variable is not defined.\n", SHELL_ENV);
  symbols = xcb_key_symbols_alloc(dpy);
  hotkeys_head = hotkeys_tail = NULL;
  progress[0] = '\0';

  snprintf(sxhkd_pid, MAXLEN, "%i", getpid());
  setenv("SXHKD_PID", sxhkd_pid, 1);
}

void cleanup(void) {
  PUTS("cleanup");
  hotkey_t *hk = hotkeys_head;
  while (hk != NULL) {
    hotkey_t *next = hk->next;
    destroy_chain(hk->chain);
    free(hk->cycle);
    free(hk);
    hk = next;
  }
  hotkeys_head = hotkeys_tail = NULL;
}

void reload_cmd(void) {
  PUTS("reload");
  cleanup();
  load_config(config_file);
  for (int i = 0; i < num_extra_confs; i++)
    load_config(extra_confs[i]);
}

void init_globals(char *cfg) {
  if (initialized) {
    reload_cmd();
    return;
  }
  int sz = strlen(cfg);
  PRINTF("Config input string length: %d\n", sz);
  config_path = sz == 0 ? NULL : cfg;

  // status_fifo must be assigned. Otherwise, the chord_t repr field will not be
  // populated.
  status_fifo = (FILE *)1;
  mapping_count = 0;
  timeout = TIMEOUT;
  grabbed = false;
  redir_fd = -1;
  abort_keysym = ESCAPE_KEYSYM;

  if (config_path == NULL) {
    char *config_home = getenv(CONFIG_HOME_ENV);
    if (config_home != NULL)
      snprintf(config_file, sizeof(config_file), "%s/%s", config_home,
               CONFIG_PATH);
    else
      snprintf(config_file, sizeof(config_file), "%s/%s/%s", getenv("HOME"),
               ".config", CONFIG_PATH);
  } else {
    snprintf(config_file, sizeof(config_file), "%s", config_path);
  }

  setup();
  get_standard_keysyms();
  get_lock_fields();
  abort_chord =
      make_chord(abort_keysym, XCB_NONE, 0, XCB_KEY_PRESS, false, false);
  load_config(config_file);

  reload = toggle_grab = bell = chained = locked = false;
  running = true;

  xcb_flush(dpy);
  initialized = true;
}
