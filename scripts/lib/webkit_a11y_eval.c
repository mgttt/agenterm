/* LD_PRELOAD helper for WebKitGTK AT-SPI Text writes.
 *
 * WebKit 2.52 registers AT-SPI Text on <textarea> but never EditableText.
 * This interposes the Wails WebView constructor, then evaluates a value
 * setter on the GTK thread. cu confirms the write with Text.GetText.
 * Never XTest / coordinates.
 */
#define _GNU_SOURCE
#include <dlfcn.h>
#include <errno.h>
#include <pthread.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/un.h>
#include <unistd.h>

typedef int gboolean;
typedef unsigned int guint;
typedef void *gpointer;
typedef gboolean (*GSourceFunc)(gpointer);

static void *g_view;
static pthread_mutex_t g_mu = PTHREAD_MUTEX_INITIALIZER;
static pthread_once_t g_once = PTHREAD_ONCE_INIT;
static guint (*g_idle_add)(GSourceFunc, gpointer);
static void (*g_run_js)(void *, const char *, void *, void *, gpointer);

struct Job {
    char *script;
};

static char *sock_path(void)
{
    const char *env = getenv("AGENTERM_WEBKIT_EVAL_SOCK");
    if (env && env[0])
        return strdup(env);
    const char *runtime = getenv("XDG_RUNTIME_DIR");
    if (runtime && runtime[0]) {
        size_t n = strlen(runtime) + 32;
        char *path = malloc(n);
        if (!path)
            return NULL;
        snprintf(path, n, "%s/agenterm-webkit-eval.sock", runtime);
        return path;
    }
    return strdup("/tmp/agenterm-webkit-eval.sock");
}

static gboolean run_job(gpointer data)
{
    struct Job *job = data;
    pthread_mutex_lock(&g_mu);
    void *view = g_view;
    pthread_mutex_unlock(&g_mu);
    if (view && g_run_js && job->script)
        g_run_js(view, job->script, NULL, NULL, NULL);
    free(job->script);
    free(job);
    return 0;
}

static int read_full(int fd, void *buf, size_t n)
{
    char *p = buf;
    size_t got = 0;
    while (got < n) {
        ssize_t r = read(fd, p + got, n - got);
        if (r == 0)
            return -1;
        if (r < 0) {
            if (errno == EINTR)
                continue;
            return -1;
        }
        got += (size_t)r;
    }
    return 0;
}

static int read_line(int fd, char *buf, size_t cap)
{
    size_t i = 0;
    while (i + 1 < cap) {
        char ch;
        if (read_full(fd, &ch, 1) != 0)
            return -1;
        if (ch == '\n') {
            buf[i] = 0;
            return 0;
        }
        buf[i++] = ch;
    }
    return -1;
}

static char *js_escape(const char *in, size_t n)
{
    size_t cap = n * 6 + 8;
    char *out = malloc(cap);
    if (!out)
        return NULL;
    size_t o = 0;
    for (size_t i = 0; i < n; i++) {
        unsigned char c = (unsigned char)in[i];
        const char *rep = NULL;
        char tmp[8];
        switch (c) {
        case '\\':
            rep = "\\\\";
            break;
        case '"':
            rep = "\\\"";
            break;
        case '\n':
            rep = "\\n";
            break;
        case '\r':
            rep = "\\r";
            break;
        case '\t':
            rep = "\\t";
            break;
        case '\'':
            rep = "\\'";
            break;
        default:
            if (c < 0x20) {
                snprintf(tmp, sizeof tmp, "\\u%04x", c);
                rep = tmp;
            }
            break;
        }
        if (rep) {
            size_t rl = strlen(rep);
            if (o + rl + 1 >= cap)
                break;
            memcpy(out + o, rep, rl);
            o += rl;
        } else {
            if (o + 2 >= cap)
                break;
            out[o++] = (char)c;
        }
    }
    out[o] = 0;
    return out;
}

static char *build_script(const char *id, size_t id_len, const char *name, size_t name_len,
                          const char *text, size_t text_len)
{
    char *eid = js_escape(id ? id : "", id_len);
    char *ename = js_escape(name ? name : "", name_len);
    char *etext = js_escape(text ? text : "", text_len);
    if (!eid || !ename || !etext) {
        free(eid);
        free(ename);
        free(etext);
        return NULL;
    }
    const char *fmt =
        "(function(){"
        "var id=\"%s\";"
        "var name=\"%s\";"
        "var v=\"%s\";"
        "var el=id?document.getElementById(id):null;"
        "if(!el&&name){"
        "var nodes=document.querySelectorAll('textarea,input,[contenteditable], [contenteditable=\"true\"]');"
        "for(var i=0;i<nodes.length;i++){"
        "var n=nodes[i];"
        "var label=(n.getAttribute('aria-label')||n.getAttribute('placeholder')||n.id||'');"
        "if(label.indexOf(name)!==-1||(label&&name.indexOf(label)!==-1)){el=n;break;}"
        "}"
        "}"
        "if(!el)return 'missing';"
        "if(el.focus)el.focus();"
        "if('value' in el){"
        "var proto=Object.getPrototypeOf(el);"
        "var desc=proto&&Object.getOwnPropertyDescriptor(proto,'value');"
        "if(desc&&desc.set)desc.set.call(el,v);else el.value=v;"
        "}else{el.textContent=v;}"
        "el.dispatchEvent(new Event('input',{bubbles:true}));"
        "el.dispatchEvent(new Event('change',{bubbles:true}));"
        "return 'ok';"
        "})()";
    size_t n = strlen(fmt) + strlen(eid) + strlen(ename) + strlen(etext) + 8;
    char *script = malloc(n);
    if (script)
        snprintf(script, n, fmt, eid, ename, etext);
    free(eid);
    free(ename);
    free(etext);
    return script;
}

static int read_prefixed(int fd, char **out, size_t *out_len)
{
    char line[64];
    if (read_line(fd, line, sizeof line) != 0)
        return -1;
    unsigned long len = 0;
    if (sscanf(line, "%lu", &len) != 1 || len > 1024 * 1024)
        return -1;
    char *buf = malloc(len + 1);
    if (!buf)
        return -1;
    if (len > 0 && read_full(fd, buf, len) != 0) {
        free(buf);
        return -1;
    }
    buf[len] = 0;
    *out = buf;
    *out_len = (size_t)len;
    return 0;
}

static void handle_client(int fd)
{
    char hello[32];
    if (read_line(fd, hello, sizeof hello) != 0 || strcmp(hello, "A11YEVAL1") != 0) {
        dprintf(fd, "ERR bad-hello\n");
        return;
    }
    char *id = NULL, *name = NULL, *text = NULL;
    size_t id_len = 0, name_len = 0, text_len = 0;
    if (read_prefixed(fd, &id, &id_len) != 0 || read_prefixed(fd, &name, &name_len) != 0 ||
        read_prefixed(fd, &text, &text_len) != 0) {
        dprintf(fd, "ERR bad-frame\n");
        free(id);
        free(name);
        free(text);
        return;
    }
    pthread_mutex_lock(&g_mu);
    void *view = g_view;
    pthread_mutex_unlock(&g_mu);
    if (!view || !g_idle_add || !g_run_js) {
        dprintf(fd, "ERR no-webview\n");
        free(id);
        free(name);
        free(text);
        return;
    }
    char *script = build_script(id, id_len, name, name_len, text, text_len);
    free(id);
    free(name);
    free(text);
    if (!script) {
        dprintf(fd, "ERR oom\n");
        return;
    }
    struct Job *job = calloc(1, sizeof *job);
    if (!job) {
        free(script);
        dprintf(fd, "ERR oom\n");
        return;
    }
    job->script = script;
    g_idle_add(run_job, job);
    dprintf(fd, "OK\n");
}

static void *server_thread(void *arg)
{
    char *path = arg;
    int fd = socket(AF_UNIX, SOCK_STREAM, 0);
    if (fd < 0) {
        free(path);
        return NULL;
    }
    unlink(path);
    struct sockaddr_un addr;
    memset(&addr, 0, sizeof addr);
    addr.sun_family = AF_UNIX;
    if (strlen(path) >= sizeof addr.sun_path) {
        close(fd);
        free(path);
        return NULL;
    }
    memcpy(addr.sun_path, path, strlen(path) + 1);
    if (bind(fd, (struct sockaddr *)&addr, sizeof addr) != 0) {
        close(fd);
        free(path);
        return NULL;
    }
    chmod(path, 0600);
    if (listen(fd, 4) != 0) {
        close(fd);
        unlink(path);
        free(path);
        return NULL;
    }
    for (;;) {
        int c = accept(fd, NULL, NULL);
        if (c < 0) {
            if (errno == EINTR)
                continue;
            break;
        }
        handle_client(c);
        close(c);
    }
    close(fd);
    unlink(path);
    free(path);
    return NULL;
}

static void resolve_syms(void)
{
    g_idle_add = (guint(*)(GSourceFunc, gpointer))dlsym(RTLD_DEFAULT, "g_idle_add");
    g_run_js = (void (*)(void *, const char *, void *, void *, gpointer))dlsym(
        RTLD_DEFAULT, "webkit_web_view_run_javascript");
}

static void start_server(void)
{
    resolve_syms();
    char *path = sock_path();
    if (!path)
        return;
    pthread_t th;
    if (pthread_create(&th, NULL, server_thread, path) == 0)
        pthread_detach(th);
    else
        free(path);
}

static void remember_view(void *view)
{
    pthread_mutex_lock(&g_mu);
    g_view = view;
    pthread_mutex_unlock(&g_mu);
    pthread_once(&g_once, start_server);
}

void *webkit_web_view_new_with_user_content_manager(void *ucm)
{
    static void *(*real)(void *) = NULL;
    if (!real) {
        real = dlsym(RTLD_NEXT, "webkit_web_view_new_with_user_content_manager");
        if (!real)
            real = dlsym(RTLD_DEFAULT, "webkit_web_view_new_with_user_content_manager");
    }
    if (!real)
        return NULL;
    void *view = real(ucm);
    if (view)
        remember_view(view);
    return view;
}

void *webkit_web_view_new(void)
{
    static void *(*real)(void) = NULL;
    if (!real) {
        real = dlsym(RTLD_NEXT, "webkit_web_view_new");
        if (!real)
            real = dlsym(RTLD_DEFAULT, "webkit_web_view_new");
    }
    if (!real)
        return NULL;
    void *view = real();
    if (view)
        remember_view(view);
    return view;
}
