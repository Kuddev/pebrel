# JSON transport boundary shared by system-provided POSIX awk implementations.

# Encode OS names and paths as JSON strings, including control bytes and backslashes.
function json_string(value, result, index_, character, control) {
    result = "\""
    for (index_ = 1; index_ <= length(value); index_++) {
        character = substr(value, index_, 1)
        if (character == "\\" || character == "\"") result = result "\\" character
        else if (character in controls) result = result controls[character]
        else result = result character
    }
    return result "\""
}

# Read a fixed system file or command response without leaving a pipe/file descriptor open.
function first_line(source, command, result, status) {
    status = command ? (source | getline result) : (getline result < source)
    close(source)
    if (status <= 0) return ""
    return result
}

# Decode filesystem capacities separately so a failed df never invalidates process counters.
function disk_rows(command, line, fields, count, mount, rows, separator, failed, index_, types, mounts, filesystem, roots, sources, root, source) {
    if (probe_os == "linux") {
        while ((getline line < "/proc/mounts") > 0) {
            split(line, mounts); gsub(/\\040/, " ", mounts[2]); types[mounts[2]] = mounts[3]
        }
        close("/proc/mounts")
        while ((getline line < "/proc/self/mountinfo") > 0) {
            split(line, mounts); mount = mounts[5]; gsub(/\\040/, " ", mount)
            root = mounts[4]; gsub(/\\040/, " ", root)
            roots[mount] = root
            sources[mount] = mounts[3]
        }
        close("/proc/self/mountinfo")
    }
    command = "df -P -k"
    command | getline line
    while ((command | getline line) > 0) {
        count = split(line, fields)
        if (count < 6 || fields[2] !~ /^[0-9]+$/ || fields[4] !~ /^[0-9]+$/) {
            failed = 1
            continue
        }
        mount = line
        sub(/^[ \t]+/, "", mount)
        for (index_ = 1; index_ <= 5; index_++) sub(/^[^ \t]+[ \t]+/, "", mount)
        filesystem = mount in types ? json_string(types[mount]) : "null"
        source = mount in sources ? sources[mount] : fields[1]
        root = mount in roots ? json_string(roots[mount]) : "null"
        rows = rows separator "{\"mount\":" json_string(mount) ",\"total\":" sprintf("%.0f", fields[2] * 1024) ",\"available\":" sprintf("%.0f", fields[4] * 1024) ",\"filesystem\":" filesystem ",\"source\":" json_string(source) ",\"root\":" root ",\"system\":" (mount == "/" ? "true" : "false") "}"
        separator = ","
    }
    if (close(command) != 0 || failed) return "null"
    return "[" rows "]"
}

# Emit environment and tool availability from this guest rather than from the desktop.
function finish_sample(disks) {
    disks = disk_rows()
    printf ",\"disks\":%s,\"disk_error\":%s", disks, disks == "null" ? "\"Filesystem query failed\"" : "null"
    printf ",\"nvidia_available\":%s", system("command -v nvidia-smi >/dev/null 2>&1") == 0 ? "true" : "false"
    printf ",\"docker_available\":%s", system("command -v docker >/dev/null 2>&1") == 0 ? "true" : "false"
    printf ",\"docker_host\":%s,\"docker_context\":%s}\n", json_string(ENVIRON["DOCKER_HOST"]), json_string(ENVIRON["DOCKER_CONTEXT"])
}

BEGIN {
    for (control = 1; control < 32; control++) controls[sprintf("%c", control)] = sprintf("\\u%04x", control)
}

# Retain five CPU consumers while deterministic PID ties prevent visual row churn.
function overview_candidate(pid, cpu, path, name, start, ticks, rank, move) {
    for (rank = 1; rank <= 5; rank++) {
        if (top_pid[rank] == "" || cpu > top_cpu[rank] || (cpu == top_cpu[rank] && pid < top_pid[rank])) {
            for (move = 5; move > rank; move--) {
                top_pid[move] = top_pid[move-1]; top_cpu[move] = top_cpu[move-1]
                top_path[move] = top_path[move-1]; top_name[move] = top_name[move-1]
                top_start[move] = top_start[move-1]; top_ticks[move] = top_ticks[move-1]
            }
            top_pid[rank] = pid; top_cpu[rank] = cpu; top_path[rank] = path
            top_name[rank] = name; top_start[rank] = start; top_ticks[rank] = ticks
            return
        }
    }
}
