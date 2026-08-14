/* LD_PRELOAD helper for WebKitGTK AT-SPI Text writes and ScrollTo.
 *
 * WebKit 2.52 registers AT-SPI Text on <textarea> but never EditableText.
 * Component.ScrollTo returns true without moving geometry. This interposes
 * the Wails WebView constructor, then evaluates a value setter or
 * scrollIntoView on the GTK thread. cu confirms writes with Text.GetText
 * and scrolls with independent Component.GetExtents. Never XTest /
 * coordinates / Action scroll* / wheel.
 */
#define _GNU_SOURCE
#include <dlfcn.h>
#include <errno.h>
#include <pthread.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
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
static void *(*g_run_js_finish)(void *, void *, void **);
static void *(*g_js_get_value)(void *);
static char *(*g_jsc_to_string)(void *);
static void (*g_js_unref)(void *);
static void (*g_free)(void *);
static void (*g_error_free)(void *);

struct Job {
    char *script;
    pthread_mutex_t mu;
    pthread_cond_t cv;
    int done;
    char reply[160];
};

static char *sock_path(void)
{
    const char *env = getenv("PLATFORM_WEBKIT_EVAL_SOCK");
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

static void finish_job(struct Job *job, const char *reply)
{
    pthread_mutex_lock(&job->mu);
    if (!job->done) {
        snprintf(job->reply, sizeof job->reply, "%s", reply ? reply : "queued");
        job->done = 1;
        pthread_cond_signal(&job->cv);
    }
    pthread_mutex_unlock(&job->mu);
}

static void js_done(void *source, void *result, void *data)
{
    struct Job *job = data;
    char *text = NULL;
    void *err = NULL;
    if (g_run_js_finish && source) {
        void *js = g_run_js_finish(source, result, &err);
        if (js && g_js_get_value && g_jsc_to_string) {
            void *val = g_js_get_value(js);
            if (val)
                text = g_jsc_to_string(val);
        }
        if (js && g_js_unref)
            g_js_unref(js);
        if (err && g_error_free)
            g_error_free(err);
    }
    finish_job(job, text ? text : "queued");
    if (text && g_free)
        g_free(text);
}

static gboolean run_job(gpointer data)
{
    struct Job *job = data;
    pthread_mutex_lock(&g_mu);
    void *view = g_view;
    pthread_mutex_unlock(&g_mu);
    if (view && g_run_js && job->script) {
        if (g_run_js_finish)
            g_run_js(view, job->script, NULL, js_done, job);
        else {
            g_run_js(view, job->script, NULL, NULL, NULL);
            finish_job(job, "queued");
        }
    } else {
        finish_job(job, "no-webview");
    }
    free(job->script);
    job->script = NULL;
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

static char *build_scroll_script(const char *id, size_t id_len, const char *name, size_t name_len)
{
    char *eid = js_escape(id ? id : "", id_len);
    char *ename = js_escape(name ? name : "", name_len);
    if (!eid || !ename) {
        free(eid);
        free(ename);
        return NULL;
    }
    const char *fmt =
        "(function(){"
        "var id=\"%s\";"
        "var name=\"%s\";"
        "function labels(n){"
        "var out=[];"
        "var a=n.getAttribute&&n.getAttribute('aria-label');if(a)out.push(a);"
        "var p=n.getAttribute&&n.getAttribute('placeholder');if(p)out.push(p);"
        "var t=n.getAttribute&&n.getAttribute('title');if(t)out.push(t);"
        "if(n.id)out.push(n.id);"
        "var c=(n.textContent||'').replace(/\\s+/g,' ').trim();"
        "if(c&&c.length<200)out.push(c);"
        "return out;"
        "}"
        "function match(n){"
        "if(id&&n.id===id)return true;"
        "if(!name)return false;"
        "var ls=labels(n);"
        "for(var i=0;i<ls.length;i++){"
        "if(ls[i].indexOf(name)!==-1||name.indexOf(ls[i])!==-1)return true;"
        "}"
        "return false;"
        "}"
        "function walk(root,acc){"
        "if(!root)return;"
        "if(root.querySelectorAll){"
        "var nodes=root.querySelectorAll('*');"
        "for(var i=0;i<nodes.length;i++){"
        "acc.push(nodes[i]);"
        "if(nodes[i].shadowRoot)walk(nodes[i].shadowRoot,acc);"
        "}"
        "}"
        "}"
        "var el=id?document.getElementById(id):null;"
        "if(!el){"
        "var acc=[];walk(document,acc);"
        "for(var i=0;i<acc.length;i++){if(match(acc[i])){el=acc[i];break;}}"
        "}"
        "if(!el)return 'missing';"
        "if(el.focus)try{el.focus({preventScroll:false});}catch(e0){try{el.focus();}catch(e00){}}"
        "var before=el.getBoundingClientRect?el.getBoundingClientRect():null;"
        "try{el.scrollIntoView({block:'start',inline:'nearest',behavior:'instant'});}"
        "catch(e1){try{el.scrollIntoView(true);}catch(e2){}}"
        "function align(node){"
        "var p=node.parentElement;"
        "while(p){"
        "if(p.scrollHeight>p.clientHeight+4||p.scrollWidth>p.clientWidth+4){"
        "var pr=p.getBoundingClientRect();"
        "var nr=node.getBoundingClientRect();"
        "p.scrollTop+=nr.top-pr.top;"
        "}"
        "p=p.parentElement;"
        "}"
        "var se=document.scrollingElement||document.documentElement;"
        "if(se){var r=node.getBoundingClientRect();if(Math.abs(r.top)>1)se.scrollTop+=r.top;}"
        "}"
        "align(el);"
        "var after=el.getBoundingClientRect?el.getBoundingClientRect():null;"
        "var bt=before?Math.round(before.top):0;"
        "var at=after?Math.round(after.top):0;"
        "return 'ok:'+bt+':'+at;"
        "})()";
    size_t n = strlen(fmt) + strlen(eid) + strlen(ename) + 8;
    char *script = malloc(n);
    if (script)
        snprintf(script, n, fmt, eid, ename);
    free(eid);
    free(ename);
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

static struct Job *queue_script(char *script)
{
    if (!script)
        return NULL;
    struct Job *job = calloc(1, sizeof *job);
    if (!job) {
        free(script);
        return NULL;
    }
    job->script = script;
    pthread_mutex_init(&job->mu, NULL);
    pthread_cond_init(&job->cv, NULL);
    g_idle_add(run_job, job);
    return job;
}

static int wait_job(struct Job *job, char *out, size_t out_len)
{
    struct timespec ts;
    clock_gettime(CLOCK_REALTIME, &ts);
    ts.tv_sec += 1;
    pthread_mutex_lock(&job->mu);
    while (!job->done) {
        if (pthread_cond_timedwait(&job->cv, &job->mu, &ts) != 0)
            break;
    }
    int done = job->done;
    snprintf(out, out_len, "%s", done ? job->reply : "timeout");
    pthread_mutex_unlock(&job->mu);
    return done;
}

static void handle_client(int fd)
{
    char hello[32];
    if (read_line(fd, hello, sizeof hello) != 0) {
        dprintf(fd, "ERR bad-hello\n");
        return;
    }
    int is_eval = strcmp(hello, "A11YEVAL1") == 0;
    int is_scroll = strcmp(hello, "A11YSCROLL1") == 0;
    if (!is_eval && !is_scroll) {
        dprintf(fd, "ERR bad-hello\n");
        return;
    }
    char *id = NULL, *name = NULL, *text = NULL;
    size_t id_len = 0, name_len = 0, text_len = 0;
    if (read_prefixed(fd, &id, &id_len) != 0 || read_prefixed(fd, &name, &name_len) != 0) {
        dprintf(fd, "ERR bad-frame\n");
        free(id);
        free(name);
        return;
    }
    if (is_eval && read_prefixed(fd, &text, &text_len) != 0) {
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
    char *script = is_scroll ? build_scroll_script(id, id_len, name, name_len)
                             : build_script(id, id_len, name, name_len, text, text_len);
    free(id);
    free(name);
    free(text);
    if (!script) {
        dprintf(fd, "ERR oom\n");
        return;
    }
    struct Job *job = queue_script(script);
    if (!job) {
        dprintf(fd, "ERR oom\n");
        return;
    }
    char js_reply[160];
    int done = wait_job(job, js_reply, sizeof js_reply);
    if (!done) {
        dprintf(fd, "OK queued\n");
        return;
    }
    pthread_mutex_destroy(&job->mu);
    pthread_cond_destroy(&job->cv);
    free(job);
    if (strncmp(js_reply, "missing", 7) == 0) {
        dprintf(fd, "ERR missing\n");
        return;
    }
    if (strncmp(js_reply, "no-webview", 10) == 0) {
        dprintf(fd, "ERR no-webview\n");
        return;
    }
    dprintf(fd, "OK %s\n", js_reply);
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
    g_run_js_finish = (void *(*)(void *, void *, void **))dlsym(
        RTLD_DEFAULT, "webkit_web_view_run_javascript_finish");
    g_js_get_value =
        (void *(*)(void *))dlsym(RTLD_DEFAULT, "webkit_javascript_result_get_js_value");
    g_jsc_to_string = (char *(*)(void *))dlsym(RTLD_DEFAULT, "jsc_value_to_string");
    g_js_unref = (void (*)(void *))dlsym(RTLD_DEFAULT, "webkit_javascript_result_unref");
    g_free = (void (*)(void *))dlsym(RTLD_DEFAULT, "g_free");
    g_error_free = (void (*)(void *))dlsym(RTLD_DEFAULT, "g_error_free");
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
