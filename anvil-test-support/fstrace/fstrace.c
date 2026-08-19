/*
 * fstrace — a syscall recorder for the hearth port coverage instrument.
 *
 * C-d.1 round 8. Seven rounds of this track each closed an enumerated set and
 * each was defeated by a noun nobody wrote down: a module a list did not name,
 * an address form a matrix did not carry, and finally a FILESYSTEM NODE the
 * fixture never created. The 249-cell behavioural matrix of round 7 was a real
 * criterion and it still could not see `transitions/`, because a column can
 * only vary a node the fixture builds.
 *
 * So the coverage predicate stops being anybody's reading of the code and
 * becomes the code's own SYSCALLS. This library is loaded into the probe
 * process (`DYLD_INSERT_LIBRARIES` on macOS, `LD_PRELOAD` on Linux) and appends
 * one line per filesystem lookup to `$FSTRACE_OUT`:
 *
 *     <op>\t<path>\n
 *
 * The instrument then fails if any path the ports actually looked at is a path
 * no matrix row takes a mode off. "Nobody thought of this noun" becomes a test
 * failure instead of a silent hole.
 *
 * It records LOOKUPS, not successes: `stat` of a path that does not exist is
 * still a dependency on that path, and the `transitions/` hole is exactly that
 * shape — read via `read_dir` on every fold, absent in every fixture, its
 * `Err(_) => Vec::new()` arm executed 249 times and green every time.
 *
 * No filtering happens here. The recorder is deliberately dumb; the instrument
 * filters to the fixture subtree, because a filter written in C is a place for
 * a hole to hide.
 */

#include <stdio.h>
#include <stdlib.h>
#include <stdarg.h>
#include <sys/stat.h>
#include <dirent.h>
#include <fcntl.h>
#include <unistd.h>

static void fstrace_record(const char *op, const char *path) {
    const char *out = getenv("FSTRACE_OUT");
    if (out == NULL || path == NULL) {
        return;
    }
    /* Append-and-close per record: the probe is a single short-lived process
     * and correctness under an interposed `open` matters more than speed. */
    FILE *f = fopen(out, "a");
    if (f == NULL) {
        return;
    }
    fprintf(f, "%s\t%s\n", op, path);
    fclose(f);
}

#ifdef __APPLE__

/* macOS two-level namespace: a plain symbol definition does NOT interpose,
 * because libstd's references bind directly to libSystem. `__DATA,__interpose`
 * is the supported mechanism and it works on an unsigned cargo test binary. */
typedef struct fstrace_interpose_s {
    const void *replacement;
    const void *original;
} fstrace_interpose_t;

static int fstrace_stat(const char *path, struct stat *buf) {
    fstrace_record("stat", path);
    return stat(path, buf);
}

static int fstrace_lstat(const char *path, struct stat *buf) {
    fstrace_record("lstat", path);
    return lstat(path, buf);
}

static DIR *fstrace_opendir(const char *path) {
    fstrace_record("opendir", path);
    return opendir(path);
}

/* The READ/WRITE distinction is load-bearing, so it is taken from the FLAGS
 * rather than from the path's spelling. This class is "an unreadable input read
 * as an empty one"; the temp sibling `atomic_write` creates is an output, not a
 * dependency, and requiring a matrix row to take a mode off a file the code
 * itself just made would be noise that teaches nobody to look anywhere. */
static int fstrace_open(const char *path, int flags, ...) {
    mode_t mode = 0;
    va_list ap;
    va_start(ap, flags);
    if (flags & O_CREAT) {
        mode = (mode_t)va_arg(ap, int);
    }
    va_end(ap);
    fstrace_record((flags & O_ACCMODE) == O_RDONLY ? "open" : "openw", path);
    return open(path, flags, mode);
}

static int fstrace_access(const char *path, int amode) {
    fstrace_record("access", path);
    return access(path, amode);
}

__attribute__((used)) static const fstrace_interpose_t fstrace_interposers[]
__attribute__((section("__DATA,__interpose"))) = {
    { (const void *)fstrace_stat,    (const void *)stat },
    { (const void *)fstrace_lstat,   (const void *)lstat },
    { (const void *)fstrace_opendir, (const void *)opendir },
    { (const void *)fstrace_open,    (const void *)open },
    { (const void *)fstrace_access,  (const void *)access },
};

#else /* glibc / LD_PRELOAD */

#define _GNU_SOURCE
#include <dlfcn.h>

int stat(const char *path, struct stat *buf) {
    static int (*real)(const char *, struct stat *);
    if (!real) real = dlsym(RTLD_NEXT, "stat");
    fstrace_record("stat", path);
    return real(path, buf);
}

int lstat(const char *path, struct stat *buf) {
    static int (*real)(const char *, struct stat *);
    if (!real) real = dlsym(RTLD_NEXT, "lstat");
    fstrace_record("lstat", path);
    return real(path, buf);
}

DIR *opendir(const char *path) {
    static DIR *(*real)(const char *);
    if (!real) real = dlsym(RTLD_NEXT, "opendir");
    fstrace_record("opendir", path);
    return real(path);
}

int open(const char *path, int flags, ...) {
    static int (*real)(const char *, int, mode_t);
    mode_t mode = 0;
    va_list ap;
    va_start(ap, flags);
    if (flags & O_CREAT) mode = (mode_t)va_arg(ap, int);
    va_end(ap);
    if (!real) real = dlsym(RTLD_NEXT, "open");
    fstrace_record((flags & O_ACCMODE) == O_RDONLY ? "open" : "openw", path);
    return real(path, flags, mode);
}

int access(const char *path, int amode) {
    static int (*real)(const char *, int);
    if (!real) real = dlsym(RTLD_NEXT, "access");
    fstrace_record("access", path);
    return real(path, amode);
}

#endif
