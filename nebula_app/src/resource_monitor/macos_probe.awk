# macOS counters use its shipped sysctl, vm_stat, top, ps and df tools.

# Convert ps cumulative CPU time, whose last field can contain fractional seconds, to milliseconds.
function cpu_milliseconds(value, fields, count, days, time_value, part) {
    days = 0
    if (index(value, "-")) {
        split(value, fields, "-")
        days = fields[1]
        value = fields[2]
    }
    count = split(value, fields, ":")
    time_value = 0
    for (part = 1; part <= count; part++) time_value = time_value * 60 + fields[part]
    return sprintf("%.0f", (days * 86400 + time_value) * 1000)
}

BEGIN {
    boot = first_line("sysctl -n kern.boottime", 1)
    cores = first_line("sysctl -n hw.logicalcpu", 1) + 0
    memory_total = first_line("sysctl -n hw.memsize", 1) + 0
    command = "vm_stat"
    while ((command | getline line) > 0) {
        if (line ~ /page size of/) {
            sub(/^.*page size of /, "", line)
            page_size = line + 0
        } else {
            split(line, fields, ":")
            if (fields[1] == "Pages active") { active = fields[2] + 0; active_seen = 1 }
            if (fields[1] == "Pages wired down") wired = fields[2] + 0
            if (fields[1] == "Pages occupied by compressor") compressed = fields[2] + 0
        }
    }
    if (close(command) != 0 || !active_seen || page_size <= 0 || cores < 1 || memory_total <= 0 || boot == "") exit 2
    memory_used = (active + wired + compressed) * page_size
    if (memory_used > memory_total) memory_used = memory_total
    # Compact rankings retain an initial CPU/creation snapshot only within this invocation.
    if (!details) {
        initial_timestamp = first_line("date +%s", 1) + 0
        command = "ps -axo pid=,lstart=,time="
        while ((command | getline line) > 0) {
            count = split(line, fields)
            if (count < 7 || fields[1] !~ /^[0-9]+$/) continue
            initial_ticks[fields[1]] = cpu_milliseconds(fields[7])
            initial_start[fields[1]] = fields[2] " " fields[3] " " fields[4] " " fields[5] " " fields[6]
        }
        if (close(command) != 0) exit 2
    }
    # top provides an interval host percentage; ps CPU deltas use cumulative milliseconds.
    command = "top -l 2 -s 1 -n 0"
    cpu = "null"
    cpu_user = "null"; cpu_system = "null"
    while ((command | getline line) > 0) {
        if (line ~ /^CPU usage:/ && line ~ /% idle/) {
            if (split(line, components, ",") == 3) {
                user = components[1]; sub(/^CPU usage:[ \t]*/, "", user)
                kernel = components[2]; sub(/^[ \t]*/, "", kernel)
                if (user ~ /^[0-9]+([.][0-9]+)?% user$/ && kernel ~ /^[0-9]+([.][0-9]+)?% sys$/ && user + 0 <= 100 && kernel + 0 <= 100) {
                    cpu_user = sprintf("%.3f", user + 0); cpu_system = sprintf("%.3f", kernel + 0)
                }
            }
            sub(/^.*,[ \t]*/, "", line)
            sub(/%.*$/, "", line)
            if (line + 0 >= 0 && line + 0 <= 100) cpu = sprintf("%.3f", 100 - line)
        }
    }
    if (close(command) != 0) { cpu = "null"; cpu_user = "null"; cpu_system = "null" }
    timestamp = first_line("date +%s", 1) + 0
    printf "{\"boot\":%s,\"timestamp\":%.0f,\"total\":null,\"idle\":null,\"cpu\":%s,\"ticks_per_second\":1000,\"cores\":%d,\"memory\":%.0f,\"total_memory\":%.0f,\"processes_complete\":%s,\"processes\":[", json_string(boot), timestamp, cpu, cores, memory_used, memory_total, details ? "true" : "false"
    command = "ps -axo pid=,lstart=,time=,rss=,comm="
    while ((command | getline line) > 0) {
        count = split(line, fields)
        if (count < 9 || fields[1] !~ /^[0-9]+$/ || fields[8] !~ /^[0-9]+$/ || fields[7] !~ /^[0-9]+([-:][0-9]+)*([.][0-9]+)?$/) continue
        if (++process_count > 16384) exit 2
        name = line
        sub(/^[ \t]+/, "", name)
        for (index_ = 1; index_ <= 8; index_++) sub(/^[^ \t]+[ \t]+/, "", name)
        start = fields[2] " " fields[3] " " fields[4] " " fields[5] " " fields[6]
        ticks = cpu_milliseconds(fields[7])
        if (details) {
            printf "%s{\"pid\":%d,\"name\":%s,\"start\":%s,\"ticks\":%s,\"memory\":%.0f,\"io_bytes\":null,\"cpu\":null}", separator, fields[1], json_string(name), json_string(start), ticks, fields[8] * 1024
            separator = ","
        } else {
            usage = -1
            if (start == initial_start[fields[1]] && ticks >= initial_ticks[fields[1]] && timestamp > initial_timestamp) usage = 100 * (ticks - initial_ticks[fields[1]]) / (1000 * (timestamp - initial_timestamp) * cores)
            if (usage > 100) usage = 100
            overview_candidate(fields[1], usage, fields[8] * 1024, name, start, ticks)
        }
    }
    if (close(command) != 0) exit 2
    if (!details) for (rank = 1; rank <= 5; rank++) {
        if (top_pid[rank] != "") {
            printf "%s{\"pid\":%d,\"name\":%s,\"start\":%s,\"ticks\":%.0f,\"memory\":%.0f,\"io_bytes\":null,\"cpu\":%s}", separator, top_pid[rank], json_string(top_name[rank]), json_string(top_start[rank]), top_ticks[rank], top_path[rank], top_cpu[rank] >= 0 ? sprintf("%.3f", top_cpu[rank]) : "null"
            separator = ","
        }
    }
    boot_seconds = boot
    sub(/^.*sec = /, "", boot_seconds); sub(/,.*$/, "", boot_seconds)
    printf "],\"summary\":{\"name\":\"macOS\",\"version\":%s,\"uptime\":%s,\"free\":%.0f,\"cache\":null,\"cpu_cores\":[],\"cpu_user\":%s,\"cpu_system\":%s,\"network\":null}", json_string(first_line("sw_vers -productVersion", 1)), (boot_seconds + 0 > 0 ? sprintf("%.0f", timestamp - boot_seconds) : "null"), memory_total - memory_used, cpu_user, cpu_system
    finish_sample()
    exit
}
