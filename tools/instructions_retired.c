// macOS process instruction counter, including every worker thread.
// cc -O2 tools/instructions_retired.c -o /tmp/instructions_retired
// /tmp/instructions_retired /absolute/path/to/program [args...]
// The child is observed while still a zombie, before waitpid releases its rusage.
// Counts cover that process, not subprocesses. Results go to stderr.
#include <errno.h>
#include <inttypes.h>
#include <libproc.h>
#include <stdio.h>
#include <sys/resource.h>
#include <sys/wait.h>
#include <unistd.h>

int main(int argc, char **argv) {
    if (argc < 2) {
        fprintf(stderr, "usage: %s PROGRAM [args...]\n", argv[0]);
        return 2;
    }
    pid_t child = fork();
    if (child < 0) {
        perror("fork");
        return 1;
    }
    if (child == 0) {
        execvp(argv[1], argv + 1);
        perror("execvp");
        _exit(127);
    }
    siginfo_t info;
    int observed;
    do {
        observed = waitid(P_PID, child, &info, WEXITED | WNOWAIT);
    } while (observed < 0 && errno == EINTR);
    struct rusage_info_v4 usage = {0};
    int counted = observed == 0
        ? proc_pid_rusage(child, RUSAGE_INFO_V4, (rusage_info_t *)&usage) : -1;
    if (counted < 0) {
        perror("instruction counter");
    } else {
        fprintf(stderr, "{\"instructions_retired\":%" PRIu64 ",\"cycles\":%" PRIu64 "}\n",
                usage.ri_instructions, usage.ri_cycles);
    }
    int status;
    pid_t waited;
    do {
        waited = waitpid(child, &status, 0);
    } while (waited < 0 && errno == EINTR);
    if (waited < 0 || counted < 0 || usage.ri_instructions == 0) {
        return 1;
    }
    return WIFEXITED(status) ? WEXITSTATUS(status) : 128 + WTERMSIG(status);
}
