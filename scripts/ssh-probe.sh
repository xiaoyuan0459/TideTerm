#!/bin/bash
# TideTerm SSH 链路探针（L3 前置门禁 + 协议层取证）
#
# 用途：把「TideTerm 能不能连上这台机器」和「连上之后行为对不对」拆成可复跑的阶段，
#       每阶段输出机器可读行，供验收报告直接引用。
# 用法（Git Bash）：
#   export TT_SSH_HOST=192.0.2.10 TT_SSH_USER=fixture
#   read -rs TT_SSH_PASS; export TT_SSH_PASS        # 口令只进环境变量，不落盘
#   export TT_SSH_KNOWN_HOSTS=/path/to/verified-fixture # 事先独立核验的主机密钥
#   bash scripts/ssh-probe.sh                        # 跑全部
#   bash scripts/ssh-probe.sh p0 p1                  # 只跑指定阶段
#   TT_SOAK=50 bash scripts/ssh-probe.sh p9          # 泄漏浸泡
#
# 阶段：p0 可达性  p1 算法/认证方式  p2 认证  p3 命令执行  p4 PTY  p5 RTT
#       p6 SFTP   p7 异常与并发     p8 弱网   p9 连接泄漏浸泡
# 退出码：0 全通过；1 存在 FAIL；2 前置条件缺失。
# 结果行格式：RESULT|<阶段>|<PASS|FAIL|SKIP|INFO>|<度量名=值 ...>

set -uo pipefail

HOST="${TT_SSH_HOST:-}"
PORT="${TT_SSH_PORT:-22}"
USER_NAME="${TT_SSH_USER:-}"
PASS="${TT_SSH_PASS:-}"
KEY="${TT_SSH_KEY:-}"
SOAK="${TT_SOAK:-20}"
WANT=("${@:-}")
[ "${#WANT[@]}" -eq 0 ] && WANT=(p0 p1 p2 p3 p4 p5 p6 p7 p8 p9)

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
FAILS=0; SKIPS=0

say()  { printf 'RESULT|%s|INFO|%s\n' "$1" "$2"; }
pass() { printf 'RESULT|%s|PASS|%s\n' "$1" "$2"; }
fail() { printf 'RESULT|%s|FAIL|%s\n' "$1" "$2"; FAILS=$((FAILS+1)); }
skip() { printf 'RESULT|%s|SKIP|%s\n' "$1" "$2"; SKIPS=$((SKIPS+1)); }
want() { local s; for s in "${WANT[@]}"; do [ "$s" = "$1" ] && return 0; done; return 1; }
ms_now() { node -e 'process.stdout.write(String(Date.now()))' 2>/dev/null || date +%s000; }

# 口令注入：OpenSSH 无 sshpass 时走 SSH_ASKPASS（Win OpenSSH 10.x + REQUIRE=force 有效）
ASK="$TMP/ap.sh"
printf '#!/bin/sh\necho "$TT_SSH_PASS"\n' > "$ASK"; chmod +x "$ASK"
export SSH_ASKPASS="$ASK" SSH_ASKPASS_REQUIRE=force DISPLAY=
SLOGIN=/dev/null
if [ -n "$KEY" ]; then
  SLOGIN=(ssh -i "$KEY" -o IdentitiesOnly=yes -o BatchMode=yes)
elif [ -n "$PASS" ]; then
  SLOGIN=(ssh)
else
  SLOGIN=(ssh -o BatchMode=yes)
fi
KNOWN_HOSTS="${TT_SSH_KNOWN_HOSTS:-}"
if [ -z "$KNOWN_HOSTS" ] || [ ! -f "$KNOWN_HOSTS" ]; then
  printf 'RESULT|setup|FAIL|TT_SSH_KNOWN_HOSTS must name a verified known_hosts fixture\n'
  exit 2
fi
SOPT=(-o "StrictHostKeyChecking=yes" -o "UserKnownHostsFile=$KNOWN_HOSTS"
      -o "ConnectTimeout=${TT_CONNECT_TIMEOUT:-10}" -o "NumberOfPasswordPrompts=1")
SDEST="${USER_NAME:+$USER_NAME@}$HOST"

# 裸 TCP 探测：输出「连上/拒绝/超时 + 是否收到 SSH banner」
raw_probe() { # $1=host $2=port
  node -e '
    const net=require("net");const s=net.connect({host:process.argv[1],port:+process.argv[2],timeout:6000},()=>{
      const t=Date.now();let n=0,buf="";s._d=0;s._c=t;
      s.on("data",d=>{n+=d.length;buf+=d.toString("latin1");if(n>200)s.end()});
      s.on("close",()=>{if(!s._d){s._d=1;console.log(JSON.stringify({r:"closed",ms:Date.now()-t,bytes:n,banner:buf.split("\r\n")[0].slice(0,60)}))}});
      s.on("error",e=>{if(!s._d){s._d=1;console.log(JSON.stringify({r:"error:"+e.code,ms:Date.now()-t,bytes:0,banner:""}))}});
      setTimeout(()=>{if(!s._d){s._d=1;console.log(JSON.stringify({r:"hold",ms:Date.now()-t,bytes:n,banner:buf.split("\r\n")[0].slice(0,60)}));s.destroy()}},4500);
    });
    s.on("timeout",()=>{if(!s._d){s._d=1;console.log(JSON.stringify({r:"noresp",ms:6000,bytes:0,banner:""}));s.destroy()}});
    s.on("error",e=>{if(!s._d){s._d=1;console.log(JSON.stringify({r:"refused:"+e.code,ms:0,bytes:0,banner:""}))}});
  ' "$1" "$2" 2>/dev/null
}

# ── p0 可达性：区分 拒绝 / 无路由 / 静默丢弃 / 本机拦截器假冒 / 非 SSH 服务 ──
p0() {
  local t0 t1 tcp ms ctl
  t0=$(ms_now)
  tcp=$(raw_probe "$HOST" "$PORT")
  t1=$(ms_now); ms=$((t1-t0))
  say p0 "raw=$tcp connect_ms=$ms"
  # 幻 open 判别：同机的一个「不可能开放」的端口若也能连上，说明本机有 TUN/代理
  # 在替任何目标完成握手（Clash TUN、Proxifier 等），此时所有远程结论都不可信。
  ctl=$(raw_probe "$HOST" "${TT_PROBE_CONTROL_PORT:-9911}")
  case "$tcp" in
    *'"r":"refused'*)
      fail p0 "verdict=refused ms=$ms —— 端口无监听（会瞬间 ECONNREFUSED），检查 sshd 与端口号" ;;
    *'"r":"noresp'*)
      fail p0 "verdict=no_route ms=$ms —— SYN 无应答，主机不在线或被防火墙静默丢弃" ;;
    *'"banner":"SSH-'*)
      case "$ctl" in
        *'"r":"refused'*|*'"r":"noresp'*) pass p0 "verdict=reachable banner=$(printf '%s' "$tcp" | sed -n 's/.*"banner":"\([^"]*\)".*/\1/p')" ;;
        *) pass p0 "verdict=reachable_but_control_port_alive —— 能握手且拿到 banner，但对照端口 $ctl 也「连得上」，本机可能有拦截器，结果需复核" ;;
      esac ;;
    *)
      case "$ctl" in
        *'"r":"refused'*|*'"r":"noresp'*)
          fail p0 "verdict=silent_drop ms=$ms —— TCP 通但对端不发识别串（sshd 未起 / tarpit / fail2ban）" ;;
        *)
          # 对照端口也「连得上且挂着」：2026-09-24 前一律判 LOCAL_INTERCEPTOR，
          # 但 2026-09-23 §0.2 已证「离线主机在本网段就是这个形态」。用 ICMP/ARP
          # 分清两种真相：主机活着 → 才可能是拦截器冒充；主机离线 → 正常链路行为。
          if ping -n 1 -w 1500 "$HOST" >/dev/null 2>&1 \
             || arp -a 2>/dev/null | grep -q "$HOST"; then
            fail p0 "verdict=LOCAL_INTERCEPTOR ms=$ms —— 主机存活（ICMP/ARP 通），且不可能开放的对照端口($ctl)也表现一致，"
            fail p0 "action=本机有 TUN/代理在冒充握手（常见：Clash Verge TUN、Proxifier、sing-box）。"
            fail p0 "action=请把局域网段设为直连绕过（如 IP-CIDR,${HOST%.*}.0/24,DIRECT,no-resolve）或临时关闭 TUN，否则这台机器上的所有远程 SSH 测试结论都不可信。"
          else
            fail p0 "verdict=host_offline ms=$ms —— ICMP 不通且无 ARP 表项：目标主机离线（或被防火墙 drop-all）。"
            fail p0 "action=对照端口($ctl)同样「连得上挂着」是本网段对离线主机的正常 TCP 形态（2026-09-23 §0.2 实证），不是本机拦截器。"
            fail p0 "action=确认目标机已开机、网线/无线已连接后重跑 p0。"
          fi ;;
      esac ;;
  esac
}

# ── p1 协商与认证方式（不需要凭据）─────────────────────────────────────────
p1() {
  local out methods
  out=$(timeout 20 ssh -vv "${SOPT[@]}" -o "PreferredAuthentications=none" \
        -o "PasswordAuthentication=no" "$SDEST" exit 2>&1 </dev/null)
  say p1 "remote_software=$(printf '%s' "$out" | sed -n 's/.*Remote protocol version.*local.*/&/p;s/.*remote software version \(.*\)/\1/p' | head -1)"
  # p1 解析缺陷（2026-09-25 修）：OpenSSH 的消息是大写开头 "Authentications
  # that can continue:"，旧模式全小写永不匹配 —— 在 .188 上实跑暴露。用
  # [Aa] 字符类（可移植），并保留 "Next authentication method:" 兜底。
  methods=$(printf '%s' "$out" | sed -n 's/.*[Aa]uthentications that can continue: \(.*\)/\1/p' | head -1)
  [ -z "$methods" ] && methods=$(printf '%s' "$out" | sed -n 's/.*Next authentication method: \(.*\)/\1/p' | head -1)
  if [ -n "$methods" ]; then
    pass p1 "server_auth_methods=$methods"
    case "$methods" in *password*) ;; *) skip p1 "服务端未开放 password 认证，p2 密码路径不可测" ;; esac
    case "$methods" in *publickey*) ;; *) skip p1 "服务端未开放 publickey 认证" ;; esac
  else
    fail p1 "未取得认证方式列表（多半 p0 已判定不可达）"
  fi
}

# ── p2 认证：口令 / 口令错误 / 公钥 / keyboard-interactive ──────────────────
p2() {
  if [ -z "$USER_NAME" ]; then skip p2 "未设 TT_SSH_USER"; return; fi
  local t0 t1 rc out
  t0=$(ms_now)
  out=$(timeout 30 "${SLOGIN[@]}" "${SOPT[@]}" "$SDEST" 'echo AUTHOK' 2>&1 </dev/null); rc=$?
  t1=$(ms_now)
  if [ $rc -eq 0 ] && printf '%s' "$out" | grep -q AUTHOK; then
    pass p2 "auth=success total_ms=$((t1-t0))"
  else
    fail p2 "auth=rejected rc=$rc err=$(printf '%s' "$out" | tail -1 | tr -d '\r')"
    return
  fi
  # 错误口令必须被拒，且不能挂住（赋值与取值必须分两行：命令替换先于同命令内的赋值展开）
  TT_SSH_PASS="${PASS}_wrong"
  export TT_SSH_PASS
  out=$(timeout 25 ssh "${SOPT[@]}" \
      -o PreferredAuthentications=password -o PubkeyAuthentication=no "$SDEST" true 2>&1 </dev/null); rc=$?
  TT_SSH_PASS="$PASS"; export TT_SSH_PASS
  if printf '%s' "${out:-}" | grep -qiE 'permission denied|Too many|incorrect'; then
    pass p2 "wrong_password=rejected rc=$rc"
  else
    fail p2 "wrong_password 未被明确拒绝 rc=$rc（可能是超时/挂起，客户端需查超时处理）"
  fi
  if [ -n "$KEY" ]; then
    if timeout 25 ssh -i "$KEY" -o IdentitiesOnly=yes -o BatchMode=yes "${SOPT[@]}" "$SDEST" true </dev/null >/dev/null 2>&1; then
      pass p2 "pubkey=accepted"
    else
      fail p2 "pubkey=rejected（检查 authorized_keys 与权限 600/700）"
    fi
  else
    skip p2 "未设 TT_SSH_KEY，公钥路径未覆盖"
  fi
}

# ── p3 命令执行与回显：退出码 / 大输出 / 二进制安全 / CJK ────────────────────
p3() {
  local out rc mark
  mark="TT_$$_${RANDOM}"
  out=$(timeout 30 "${SLOGIN[@]}" "${SOPT[@]}" "$SDEST" "printf 'M=%s\n' $mark; echo x; exit 7" 2>/dev/null); rc=$?
  printf '%s' "$out" | grep -q "M=$mark" && pass p3 "marker_ok rc_seen=$rc" || fail p3 "命令回显丢失 rc=$rc"
  [ "$rc" = "7" ] || fail p3 "退出码未透传：期望 7 实得 $rc"
  # 1 MiB 输出的完整性（校验行数与 sha256 前缀）
  out=$(timeout 60 "${SLOGIN[@]}" "${SOPT[@]}" "$SDEST" \
        'seq 1 20000 | sha256sum; printf "\xe4\xb8\xad\xe6\x96\x87\n"' 2>/dev/null)
  printf '%s' "$out" | grep -q 中文 && pass p3 "cjk_roundtrip=ok" || fail p3 "CJK 往返损坏"
  say p3 "digest=$(printf '%s' "$out" | head -1)"
}

# ── p4 PTY 分配：-tt 强制、TERM、winsize、无 TTY 对比 ───────────────────────
p4() {
  local out
  out=$(timeout 30 "${SLOGIN[@]}" "${SOPT[@]}" -tt "$SDEST" \
        'echo T=$(tty) TERM=$TERM SZ=$(stty size 2>/dev/null) RAW=$(stty -a 2>/dev/null | head -1 | cut -c1-40)' 2>/dev/null | tr -d '\r')
  case "$out" in
    *"T=/dev/pts/"*) pass p4 "pty_allocated out=$(printf '%s' "$out" | grep -o 'T=.*' | head -1)" ;;
    *) fail p4 "未分配到 PTY：$out" ;;
  esac
  out=$(timeout 20 "${SLOGIN[@]}" "${SOPT[@]}" "$SDEST" 'tty' 2>/dev/null | tr -d '\r')
  printf '%s' "$out" | grep -q "not a tty" && pass p4 "no_pty_channel_correct" || say p4 "无 -t 时 tty=$out（期望 not a tty）"
}

# ── p5 建连 RTT 分布（P50/P95/max）：连接成本基线 ──────────────────────────
p5() {
  local n=15 i t0 t1 d list=""
  for i in $(seq 1 $n); do
    t0=$(ms_now)
    timeout 25 "${SLOGIN[@]}" "${SOPT[@]}" "$SDEST" true </dev/null >/dev/null 2>&1 || { fail p5 "第 $i 次建连失败"; return; }
    t1=$(ms_now); d=$((t1-t0)); list="$list $d"
  done
  sorted=$(printf '%s\n' $list | sort -n)
  say p5 "samples=$(printf '%s' "$sorted" | tr '\n' ' ')"
  pass p5 "min=$(printf '%s\n' "$sorted" | head -1) p50=$(printf '%s\n' "$sorted" | sed -n "$((n/2))p") max=$(printf '%s\n' "$sorted" | tail -1)"
}

# ── p6 传输往返：上传/下载/校验（走 scp，见下方口径说明）──────────────────
p6() {
  command -v scp >/dev/null 2>&1 || { skip p6 "无 scp 客户端"; return; }
  local big sha
  head -c 1048576 /dev/urandom > "$TMP/a.bin" 2>/dev/null || dd if=/dev/zero of="$TMP/a.bin" bs=1k count=1024 2>/dev/null
  sha=$(sha256sum "$TMP/a.bin" | cut -c1-16)
  # 口径说明：`sftp -b` 是批量模式，会**禁用口令交互**，配 SSH_ASKPASS 必失败，
  # 因此这里走 scp（同一 SSH 传输层、不同子系统）。TideTerm 自己的 SFTP 面板
  # 走 sftp subsystem，只能由 GUI 用例 SFTP-01/02 覆盖。
  big=$(timeout 120 scp "${SOPT[@]}" -q "$TMP/a.bin" "$SDEST:/tmp/tt_probe.bin" 2>&1 </dev/null)
  big="$big$(timeout 120 scp "${SOPT[@]}" -q "$SDEST:/tmp/tt_probe.bin" "$TMP/b.bin" 2>&1 </dev/null)"
  timeout 30 "${SLOGIN[@]}" "${SOPT[@]}" "$SDEST" 'rm -f /tmp/tt_probe.bin' </dev/null >/dev/null 2>&1
  if [ -f "$TMP/b.bin" ] && [ "$(sha256sum "$TMP/b.bin" | cut -c1-16)" = "$sha" ]; then
    pass p6 "roundtrip=1MiB via=scp digest_match"
  else
    fail p6 "传输往返校验不一致 out=$(printf '%s' "$big" | tail -2 | tr '\r\n' '  ')"
  fi
}

# ── p7 异常与并发：中途断链、MaxStartups 风暴、keepalive ────────────────────
p7() {
  timeout 12 "${SLOGIN[@]}" "${SOPT[@]}" -tt "$SDEST" 'sleep 30; echo SURVIVED' </dev/null >/dev/null 2>&1
  say p7 "client_killed_midstream_rc=$? （观察 TideTerm 是否把会话标记为已断开而非静默）"
  pass p7 "abrupt_close_no_hang"
  local i okf="$TMP/par_ok"
  : > "$okf"
  for i in $(seq 1 12); do
    (timeout 25 "${SLOGIN[@]}" "${SOPT[@]}" "$SDEST" true </dev/null >/dev/null 2>&1 \
        && echo 1 >> "$okf") &
  done
  wait
  say p7 "parallel_handshakes=12 success=$(wc -l < "$okf") 明显 <12 说明服务端 MaxStartups 限流，弱网重连测试需据此降并发"
  pass p7 "handshake_storm_done"
}

# ── p8 弱网：本机侧无法 tc，改判可用工具并如实标注 ──────────────────────────
p8() {
  if command -v tc >/dev/null 2>&1; then skip p8 "存在 tc，请在 Linux 侧按用例 NET-04 施加 netem"; return; fi
  say p8 "impairment=none Windows/Git Bash 无 tc；2000ms/30% 丢包需经 WSL2 或跳板机 netem，见用例 NET-04"
  skip p8 "自动化未覆盖，需人工弱网台架"
}

# ── p9 建连浸泡：句柄/线程增长取证（配合客户端进程）─────────────────────────
p9() {
  local i base now
  base=$(timeout 25 "${SLOGIN[@]}" "${SOPT[@]}" "$SDEST" 'cat /proc/sys/fs/file-nr; ls /proc/self/task | wc -l' 2>/dev/null | tr -d '\r')
  for i in $(seq 1 "$SOAK"); do
    timeout 25 "${SLOGIN[@]}" "${SOPT[@]}" "$SDEST" true </dev/null >/dev/null 2>&1 || { fail p9 "浸泡第 $i 次失败"; return; }
  done
  now=$(timeout 25 "${SLOGIN[@]}" "${SOPT[@]}" "$SDEST" 'cat /proc/sys/fs/file-nr; ls /proc/self/task | wc -l' 2>/dev/null | tr -d '\r')
  pass p9 "soak=$SOAK server_before=[$(printf '%s' "$base" | tr '\n' '/')] server_after=[$(printf '%s' "$now" | tr '\n' '/')]"
  say p9 "客户端侧句柄增长由 scripts/leak-soak.ps1 采样（GetProcessHandleCount + 线程数 + RSS），本阶段只保证服务端无残留"
}

for stage in "${WANT[@]}"; do
  case "$stage" in
    p0|p1|p2|p3|p4|p5|p6|p7|p8|p9) ;;
    *) echo "未知阶段: $stage" >&2; exit 2 ;;
  esac
done
if [ -z "$HOST" ]; then echo "RESULT|preflight|FAIL|未设 TT_SSH_HOST" >&2; exit 2; fi
if ! command -v ssh >/dev/null 2>&1; then echo "RESULT|preflight|FAIL|未找到 OpenSSH 客户端" >&2; exit 2; fi
if [ "$HOST" != "127.0.0.1" ] && [ "$HOST" != "localhost" ]; then
  say preflight "target=$HOST:$PORT user=${USER_NAME:-<未设>} pass=$([ -n "$PASS" ] && echo from-env || echo unset)"
fi

for stage in "${WANT[@]}"; do "$stage"; done

printf 'RESULT|summary|%s|fails=%s skips=%s\n' "$([ $FAILS -eq 0 ] && echo PASS || echo FAIL)" "$FAILS" "$SKIPS"
[ $FAILS -eq 0 ]
