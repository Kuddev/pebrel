# Linux counters come from procfs; no language runtime or external proc parser is installed.
# Capture process CPU identities without reading command lines or per-process I/O.
function process_counters(path, stat, opening, ending, position, count, fields) {
    stat = first_line(path, 0)
    opening = index(stat, "(")
    ending = 0
    for (position = length(stat); position > opening; position--) {
        if (substr(stat, position, 1) == ")") { ending = position; break }
    }
    if (!opening || !ending || split(substr(stat, ending + 2), fields) < 22) return 0
    process_pid = substr(stat, 1, opening - 1) + 0
    process_start = fields[20]
    process_ticks = fields[12] + fields[13]
    process_name = substr(stat, opening + 1, ending - opening - 1)
    return process_pid > 0 && process_start ~ /^[0-9]+$/
}

# Resident memory and optional I/O are read only for processes included in the response.
function emit_process(path, pid, name, start, ticks, cpu, status_path, line, fields, resident, io_path, reads, writes, io) {
    status_path = path; sub(/stat$/, "status", status_path)
    while ((getline line < status_path) > 0) {
        split(line, fields)
        if (fields[1] == "VmRSS:") resident = fields[2] * 1024
    }
    close(status_path)
    io = "null"
    if (details) {
        io_path = path; sub(/stat$/, "io", io_path)
        while ((getline line < io_path) > 0) {
            split(line, fields)
            if (fields[1] == "read_bytes:") reads = fields[2]
            if (fields[1] == "write_bytes:") writes = fields[2]
        }
        close(io_path)
        if (reads != "" && writes != "") io = sprintf("%.0f", reads + writes)
    }
    printf "%s{\"pid\":%d,\"name\":%s,\"start\":%s,\"ticks\":%.0f,\"memory\":%.0f,\"io_bytes\":%s,\"cpu\":%s}", separator, pid, json_string(name), json_string(start), ticks, resident, io, cpu
    separator = ","
}

BEGIN {
    probe_os = "linux"
    if (ARGC > 16385) exit 2
    # Overview CPU ranking uses an interval entirely on the guest, avoiding a full PID response.
    if (!details) {
        for (argument = 1; argument < ARGC; argument++) {
            if (process_counters(ARGV[argument])) {
                old_ticks[process_pid] = process_ticks; old_start[process_pid] = process_start
            }
        }
    }
    while ((getline line < "/proc/stat") > 0) {
        split(line, fields)
        if (fields[1] ~ /^cpu[0-9]*$/) {
            for (index_ = 2; index_ <= 9; index_++) core_total[fields[1]] += fields[index_]
            core_idle[fields[1]] = fields[5] + fields[6]
            if (fields[1] == "cpu") { old_user = fields[2] + fields[3]; old_system = fields[4] + fields[7] + fields[8] + fields[9] }
        }
    }
    close("/proc/stat")
    if (system("sleep 0.25") != 0) exit 2
    while ((getline line < "/proc/stat") > 0) {
        split(line, fields)
        if (fields[1] ~ /^cpu[0-9]*$/) {
            current_total = 0
            for (index_ = 2; index_ <= 9; index_++) current_total += fields[index_]
            delta = current_total - core_total[fields[1]]
            idle_delta = fields[5] + fields[6] - core_idle[fields[1]]
            core_usage[fields[1]] = delta > 0 && idle_delta >= 0 && idle_delta <= delta ? sprintf("%.3f", 100 * (1 - idle_delta / delta)) : "null"
            if (fields[1] == "cpu") {
                total = current_total; idle = fields[5] + fields[6]; total_delta = delta
                user_delta = fields[2] + fields[3] - old_user
                system_delta = fields[4] + fields[7] + fields[8] + fields[9] - old_system
                cpu_user = delta > 0 && user_delta >= 0 && user_delta <= delta ? sprintf("%.3f", 100 * user_delta / delta) : "null"
                cpu_system = delta > 0 && system_delta >= 0 && system_delta <= delta ? sprintf("%.3f", 100 * system_delta / delta) : "null"
            }
            else cores++
        }
    }
    close("/proc/stat")
    while ((getline line < "/proc/meminfo") > 0) {
        split(line, fields)
        if (fields[1] == "MemTotal:") memory_total = fields[2] * 1024
        if (fields[1] == "MemFree:") memory_free = fields[2] * 1024
        if (fields[1] == "Buffers:" || fields[1] == "Cached:" || fields[1] == "SReclaimable:") memory_cache += fields[2] * 1024
    }
    close("/proc/meminfo")
    memory_used = memory_total - memory_free - memory_cache
    if (memory_used < 0) { memory_used = 0; memory_cache = memory_total - memory_free }
    boot = first_line("/proc/sys/kernel/random/boot_id", 0)
    split(first_line("/proc/uptime", 0), uptime)
    if (cores < 1 || memory_total <= 0 || boot == "" || uptime[1] <= 0) exit 2
    printf "{\"boot\":%s,\"timestamp\":%s,\"total\":%.0f,\"idle\":%.0f,\"cpu\":%s,\"ticks_per_second\":null,\"cores\":%d,\"memory\":%.0f,\"total_memory\":%.0f,\"processes_complete\":%s,\"processes\":[", json_string(boot), uptime[1], total, idle, core_usage["cpu"], cores, memory_used, memory_total, details ? "true" : "false"
    for (argument = 1; argument < ARGC; argument++) {
        path = ARGV[argument]
        if (!process_counters(path)) continue
        if (details) emit_process(path, process_pid, process_name, process_start, process_ticks, "null")
        else {
            usage = -1
            if (process_start == old_start[process_pid] && process_ticks >= old_ticks[process_pid] && total_delta > 0) usage = 100 * (process_ticks - old_ticks[process_pid]) / total_delta
            if (usage > 100) usage = 100
            overview_candidate(process_pid, usage, path, process_name, process_start, process_ticks)
        }
    }
    if (!details) for (rank = 1; rank <= 5; rank++) {
        if (top_pid[rank] != "") emit_process(top_path[rank], top_pid[rank], top_name[rank], top_start[rank], top_ticks[rank], top_cpu[rank] >= 0 ? sprintf("%.3f", top_cpu[rank]) : "null")
    }
    while ((getline line < "/etc/os-release") > 0) {
        if (line ~ /^NAME=/) { os_name = substr(line, 6); gsub(/^"|"$/, "", os_name) }
        if (line ~ /^VERSION=/) { os_version = substr(line, 9); gsub(/^"|"$/, "", os_version) }
    }
    close("/etc/os-release")
    if (os_name == "") os_name = "Linux"
    printf "],\"summary\":{\"name\":%s,\"version\":%s,\"uptime\":%.0f,\"free\":%.0f,\"cache\":%.0f,\"cpu_cores\":[", json_string(os_name), json_string(os_version), int(uptime[1]), memory_free, memory_cache
    for (index_ = 0; index_ < cores; index_++) printf "%s%s", index_ ? "," : "", core_usage["cpu" index_] == "" ? "null" : core_usage["cpu" index_]
    printf "],\"cpu_user\":%s,\"cpu_system\":%s,\"network\":%s}", cpu_user, cpu_system, linux_network()
    finish_sample()
    exit
}

# Interface-scoped cumulative counters allow the client to reject topology changes and resets.
function linux_network(line, parts, fields, interface, names, separator, rx, tx, count) {
    while ((getline line < "/proc/net/dev") > 0) {
        if (index(line, ":") == 0) continue
        split(line, parts, ":"); interface = parts[1]; gsub(/[ \t]/, "", interface)
        if (interface == "lo" || interface == "docker0" || interface ~ /^veth/) continue
        split(parts[2], fields); rx += fields[1]; tx += fields[9]
        names = names separator json_string(interface); separator = ","; count++
    }
    close("/proc/net/dev")
    if (!count) return "null"
    return "{\"interfaces\":[" names "],\"received\":" sprintf("%.0f", rx) ",\"transmitted\":" sprintf("%.0f", tx) ",\"receive_rate\":null,\"transmit_rate\":null}"
}
