// mem_snapshot —— 进程内存构成取证。
//
// 为什么需要它：`scripts/measure-startup.ps1` 只给"总共多少"，给不出"谁占的"。
// 2026-09-28 的探针矩阵显示「主窗无终端 13.8 MB → 加一个终端页 38.9 MB」，而同一时刻
// Rust 侧 `term_mem_report()` 报 历史0行/回滚0B/帧4B —— 差额整块落在 Qt/堆上，读代码
// 猜不出来源（§6 那条"双族名 QFont ⇒ 整套回退链 ~40 MB"就是靠二分才定位的）。这里把
// 两件正交的事摊开：
//   · VirtualQuery 扫全地址空间 ⇒ 按类型汇总 + 列出最大的若干提交区段（能直接认出
//     "15.4 MB ≈ 2560×1540×4" 这类整屏缓冲尺寸）；
//   · GetProcessHeaps + HeapWalk ⇒ 每个堆的存活块数、字节量、尺寸分布与最大块。
// 只在内存探针里由 `TIDETERM_MEM_SNAPSHOT` 打开；HeapWalk 在不支持的堆上会失败，那种
// 情况如实标注"不可枚举"，不静默给出看似完整的数字。
#include "cpp/chrome/mem_snapshot.h"

#include <QApplication>
#include <algorithm>
#include <array>
#include <cstdio>
#include <cstring>
#include <vector>

#ifdef Q_OS_WIN
#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>

namespace {

constexpr SIZE_T kMiB = 1024 * 1024;
constexpr int kTopRegions = 14;
constexpr int kTopBlocks = 10;

double mb(SIZE_T bytes) { return double(bytes) / double(kMiB); }

struct Chunk {
    SIZE_T size;      // 提交字节
    SIZE_T addr;
    DWORD protect;
    SIZE_T resident;  // 驻留字节；SIZE_T(-1) 表示查询失败
};

bool bigger(const Chunk &a, const Chunk &b) { return a.size > b.size; }
bool dirtier(const Chunk &a, const Chunk &b) {
    return a.resident != b.resident ? a.resident > b.resident : a.size > b.size;
}

/// 保护位缩写；只给判读需要的三类。
const char *protect_name(DWORD p) {
    if ((p & (PAGE_EXECUTE | PAGE_EXECUTE_READ | PAGE_EXECUTE_READWRITE
              | PAGE_EXECUTE_WRITECOPY)) != 0) {
        return (p & (PAGE_READWRITE | PAGE_WRITECOPY | PAGE_EXECUTE_READWRITE
                     | PAGE_EXECUTE_WRITECOPY))
                   ? "RXW"
                   : "R-X";
    }
    switch (p & 0xFF) {
        case PAGE_READWRITE:
        case PAGE_WRITECOPY:
        case PAGE_EXECUTE_WRITECOPY: return "RW-";
        case PAGE_READONLY: return "R--";
        case PAGE_NOACCESS: return "---";
        default: return "?";
    }
}

// 有界、栈上统计：HeapWalk 期间不能为每个 BUSY 块分配记录，否则会枚举到探针自己。
template <size_t N> struct TopChunks {
    std::array<Chunk, N> items{};
    size_t count = 0;

    void add(const Chunk &c, bool by_resident = false) {
        const auto better = by_resident ? dirtier : bigger;
        size_t i = 0;
        while (i < count && !better(c, items[i])) ++i;
        if (i == N) return;
        if (count < N) ++count;
        for (size_t j = count - 1; j > i; --j) items[j] = items[j - 1];
        items[i] = c;
    }
};

/// 大堆块（≥256 KB）内容指纹：头部 hex + 头部可打印字符采样 + 首 8 KB 零字节密度 +
/// 块内字符串段采样（≥10 字符可打印段取最长三条，各截 96 字符）。尺寸公式对不上号时
/// （比如 2026-09-28 那两块 1.25 MB / 2.00 MB），靠内容特征区分「全是零的容量缓冲」
/// 「装了文本/翻译/样式表的缓冲」「结构体数组」——块来自 HeapWalk 的 BUSY 枚举、
/// dump 跑在探针单线程路径上，直接读是安全的。
void fingerprint(SIZE_T addr, SIZE_T size) {
    const unsigned char *p = reinterpret_cast<const unsigned char *>(addr);
    char hex[3 * 16 + 1];
    char ascii[33];
    size_t zeros = 0;
    const size_t probe = size < 8192 ? size : 8192;
    for (size_t i = 0; i < probe; ++i) {
        if (p[i] == 0) ++zeros;
    }
    for (int i = 0; i < 16; ++i) {
        std::snprintf(hex + 3 * i, 4, "%02X ", p[i]);
        ascii[i] = p[i] >= 0x20 && p[i] < 0x7F ? char(p[i]) : '.';
    }
    ascii[16] = '\0';
    std::printf("[mem]       指纹 零密度=%zu/%zu  ascii=\"%s\"  hex=%s\n", zeros, probe, ascii,
                hex);
    // 字符串段采样：只扫前 128 KB（指纹不是全文检索）；魔串速判放最前。
    const size_t scan = size < 128 * 1024 ? size : 128 * 1024;
    const char *magic[] = {"PNG", "<?xml", "QTQT", "DSIG", "\x00\x01\x00\x00"};
    for (const char *m : magic) {
        const size_t ml = std::strlen(m) == 0 ? 2 : std::strlen(m);
        bool hit = false;
        for (size_t i = 0; i + ml <= scan && !hit; ++i) {
            hit = std::memcmp(p + i, m, ml) == 0;
        }
        if (hit) std::printf("[mem]       含魔串 %.10s\n", m[0] >= ' ' ? m : "(binary)");
    }
    struct Seg {
        size_t off, len;
    };
    Seg best[3] = {{0, 0}, {0, 0}, {0, 0}};
    size_t run_off = 0, run_len = 0;
    for (size_t i = 0; i <= scan; ++i) {
        const bool printable = i < scan && p[i] >= 0x20 && p[i] < 0x7F;
        if (printable) {
            if (run_len == 0) run_off = i;
            ++run_len;
        } else {
            if (run_len >= 10) {
                for (Seg &b : best) {
                    if (run_len > b.len) {
                        const Seg keep = b;
                        b = Seg{run_off, run_len};
                        // 顶掉的段往下顺移一格，保住次长。
                        for (Seg *n = &b + 1; n < best + 3 && keep.len > n->len; ++n) {
                            const Seg keep2 = *n;
                            *n = keep;
                            if (keep2.len == 0) break;
                        }
                        break;
                    }
                }
            }
            run_len = 0;
        }
    }
    for (const Seg &b : best) {
        if (b.len == 0) continue;
        char text[97];
        const size_t n = b.len < 96 ? b.len : 96;
        std::memcpy(text, p + b.off, n);
        text[n] = '\0';
        std::printf("[mem]       字符串段 @+%zu len=%zu \"%s\"\n", b.off, b.len, text);
    }
    // sfnt 字体识别：头部是 TrueType/OTF/CFF collection 时，dump 表目录 tag 列表，
    // 并在块内搜常见字体名的 UTF-16BE 编码（name 表用 UTF-16BE），直接认出哪张脸。
    const bool is_sfnt = (size >= 40 && std::memcmp(p, "\x00\x01\x00\x00", 4) == 0) ||
                         (size >= 40 && std::memcmp(p, "OTTO", 4) == 0) ||
                         (size >= 40 && std::memcmp(p, "ttcf", 4) == 0);
    if (is_sfnt) {
        const unsigned tables = (unsigned)p[4] << 8 | p[5];
        std::printf("[mem]       sfnt 字体 tables=%u:", tables);
        for (unsigned t = 0; t < tables && t < 24; ++t) {
            const unsigned char *e = p + 12 + 16 * size_t(t);
            std::printf(" %.4s", reinterpret_cast<const char *>(e));
        }
        std::printf("\n");
        // 解析 name 表（nameID=1 family / nameID=4 full / nameID=6 postscript，Windows
        // UTF-16BE 记录），直接认出这张脸 —— 比 hex/魔串猜测可靠。
        for (unsigned t = 0; t < tables; ++t) {
            const unsigned char *e = p + 12 + 16 * size_t(t);
            if (std::memcmp(e, "name", 4) != 0) continue;
            const size_t off = (size_t(e[8]) << 24) | (size_t(e[9]) << 16) |
                               (size_t(e[10]) << 8) | e[11];
            if (off + 6 > size) break;
            const unsigned char *np = p + off;
            const unsigned count = (unsigned(np[2]) << 8) | np[3];
            for (unsigned r = 0; r < count && r < 64; ++r) {
                const unsigned char *rec = np + 6 + 12 * size_t(r);
                const unsigned pid = (unsigned(rec[0]) << 8) | rec[1];
                const unsigned nid = (unsigned(rec[6]) << 8) | rec[7];
                const unsigned len = (unsigned(rec[8]) << 8) | rec[9];
                const unsigned roff = (unsigned(rec[10]) << 8) | rec[11];
                if (pid != 3 && pid != 0) continue;
                if (nid != 1 && nid != 4 && nid != 6) continue;
                const size_t so = off + 6 + 12 * size_t(count) + roff;
                if (so + len > size || len == 0 || len > 128) continue;
                char text[130];
                for (unsigned i = 0; i < len / 2; ++i) {
                    const unsigned ch = (unsigned(p[so + 2 * i + 1])) |
                                         (unsigned(p[so + 2 * i]) << 8);
                    text[i] = ch >= 0x20 && ch < 0x7F ? char(ch) : '?';
                }
                text[len / 2] = '\0';
                std::printf("[mem]       sfnt name ID%u \"%s\"\n", nid, text);
            }
            break;
        }
    }
}

/// 一行一个区段/块。字节数原样打印，便于拿 `cols×rows×sizeof` 之类的公式对号。
///   0 = 映射区段；1 = 私有区段（多打驻留，并按驻留排序）；2 = 堆内存存块。
template <size_t N> void top_line(const char *label, const TopChunks<N> &v, int kind) {
    std::printf("[mem]   %s\n", label);
    for (size_t i = 0; i < v.count; ++i) {
        const Chunk &c = v.items[i];
        switch (kind) {
            case 1: std::printf("[mem]     提交 %8.2f MiB (%9zu)  驻留 ", mb(c.size), c.size);
                    if (c.resident == SIZE_T(-1)) std::printf("n/a");
                    else std::printf("%8.2f MiB", mb(c.resident));
                    std::printf("  @0x%llX  %s\n", static_cast<unsigned long long>(c.addr),
                                protect_name(c.protect)); break;
            case 2: std::printf("[mem]     %8.2f MiB (%9zu)  @0x%llX\n", mb(c.size), c.size,
                                static_cast<unsigned long long>(c.addr));
                      if (c.size >= 256 * 1024) fingerprint(c.addr, c.size); break;
            default: std::printf("[mem]     %8.2f MiB (%9zu)  @0x%llX  %s\n", mb(c.size), c.size,
                                 static_cast<unsigned long long>(c.addr), protect_name(c.protect)); break;
        }
    }
}

constexpr std::array<SIZE_T, 10> kBucketLimits = {
    32, 64, 256, 1024, 4096, 16384, 65536, 262144, 1048576, 8388608};
constexpr std::array<const char *, 11> kBucketNames = {
    "0-32", "33-64", "65-256", "257B-1K", "1-4K", "4-16K", "16-64K",
    "64-256K", "256K-1M", "1-8M", ">8M"};

size_t bucket_of(SIZE_T cb) {
    return size_t(std::lower_bound(kBucketLimits.begin(), kBucketLimits.end(), cb)
                  - kBucketLimits.begin());
}

// ---- 地址空间 ----

struct VmTotals {
    SIZE_T priv = 0;
    SIZE_T priv_resident = 0;
    SIZE_T image_private_resident = 0;
    SIZE_T mapped_private_resident = 0;
    bool residency_ok = true;
    SIZE_T mapped = 0;
    SIZE_T image = 0;
    size_t regions = 0;
    TopChunks<kTopRegions> big_priv;
    TopChunks<kTopRegions> big_map;
};

// ---- 驻留页 ----
//
// 提交量对不上任务管理器那一列：`--chrome` 提交私有 28.96 MB，而任务管理器只有 9.6 MB
// —— 差的全是"已提交但没被碰过"的页。所以判"谁真的占着内存"必须逐页问驻留位。
// QueryWorkingSetEx 官方归 psapi，实际导出名 K32QueryWorkingSetEx 在 kernelbase；这里
// 按名字取址，免得为一个取证 TU 给整个二进制加 psapi.lib 链接依赖。拿不到就报 n/a，
// 不拿提交量冒充驻留量。

constexpr SIZE_T kPageSize = 4096;

using QueryWorkingSetExFn = BOOL(WINAPI *)(HANDLE, PVOID, DWORD);

QueryWorkingSetExFn query_ws_ex() {
    static const QueryWorkingSetExFn fn = [] {
        HMODULE mod = GetModuleHandleW(L"kernelbase.dll");
        return mod == nullptr
                   ? QueryWorkingSetExFn{}
                   : reinterpret_cast<QueryWorkingSetExFn>(
                         GetProcAddress(mod, "K32QueryWorkingSetEx"));
    }();
    return fn;
}

struct WsEntry {
    PVOID addr;
    ULONG_PTR attr;
};

/// 固定容量批量查页，避免大区段造成探针自身 MB 级分配。
/// MEM_IMAGE/MEM_MAPPED 的写时复制页仍保持原 Type，必须同时检查 Shared 位。
bool resident_bytes(SIZE_T base, SIZE_T size, SIZE_T *resident, SIZE_T *private_resident) {
    static const QueryWorkingSetExFn fn = query_ws_ex();
    if (fn == nullptr) return false;
    std::array<WsEntry, 1024> pages{};
    const SIZE_T n = size / kPageSize;
    for (SIZE_T offset = 0; offset < n; offset += pages.size()) {
        const size_t count = size_t(std::min<SIZE_T>(n - offset, pages.size()));
        for (size_t i = 0; i < count; ++i) {
            pages[i] = {PVOID(base + (offset + i) * kPageSize), 0};
        }
        if (!fn(GetCurrentProcess(), pages.data(), DWORD(count * sizeof(WsEntry)))) return false;
        for (size_t i = 0; i < count; ++i) {
            if (!(pages[i].attr & 1u)) continue; // bit0 = Valid
            *resident += kPageSize;
            if (!(pages[i].attr & 0x8000u)) *private_resident += kPageSize; // bit15 = Shared
        }
    }
    return true;
}

void sweep_virtual(VmTotals *t) {
    MEMORY_BASIC_INFORMATION mbi{};
    SIZE_T addr = 0;
    while (VirtualQuery((LPCVOID)addr, &mbi, sizeof(mbi)) == sizeof(mbi)) {
        if (mbi.RegionSize == 0) break;
        if (mbi.State == MEM_COMMIT) {
            ++t->regions;
            Chunk c{mbi.RegionSize, SIZE_T(mbi.BaseAddress), mbi.Protect, 0};
            SIZE_T private_resident = 0;
            if (!resident_bytes(c.addr, c.size, &c.resident, &private_resident)) {
                t->residency_ok = false;
                c.resident = SIZE_T(-1);
            }
            switch (mbi.Type) {
                case MEM_PRIVATE:
                    t->priv += c.size;
                    t->priv_resident += private_resident;
                    t->big_priv.add(c, true); break;
                case MEM_MAPPED:
                    t->mapped += c.size;
                    t->mapped_private_resident += private_resident;
                    t->big_map.add(c); break;
                case MEM_IMAGE:
                    t->image += c.size;
                    t->image_private_resident += private_resident; break;
                default: break;
            }
        }
        const SIZE_T next = SIZE_T(mbi.BaseAddress) + mbi.RegionSize;
        if (next <= addr) break;
        addr = next;
    }
}

// ---- 堆 ----

std::vector<HANDLE> list_heaps() {
    std::vector<HANDLE> heaps(64);
    for (int attempt = 0; attempt < 4; ++attempt) {
        const DWORD got = GetProcessHeaps(DWORD(heaps.size()), heaps.data());
        if (got == 0) return {};
        if (got <= heaps.size()) {
            heaps.resize(got);
            return heaps;
        }
        heaps.resize(got);
    }
    return {};
}

struct HeapStat {
    size_t blocks = 0;
    SIZE_T bytes = 0;
    bool walkable = false;
    TopChunks<kTopBlocks> top;
    std::array<SIZE_T, kBucketNames.size()> count{};
    std::array<SIZE_T, kBucketNames.size()> sum{};
};

HeapStat sweep_heap(HANDLE h) {
    HeapStat s;
    PROCESS_HEAP_ENTRY e{};
    if (!HeapLock(h)) {
        std::printf("[mem] 堆 @0x%llX HeapLock 失败（%lu）\n",
                    unsigned long long(SIZE_T(h)), GetLastError());
        return s;
    }
    while (HeapWalk(h, &e)) {
        s.walkable = true;
        if (!(e.wFlags & PROCESS_HEAP_ENTRY_BUSY)) continue;
        const SIZE_T cb = e.cbData;
        ++s.blocks;
        s.bytes += cb;
        const size_t bucket = bucket_of(cb);
        ++s.count[bucket];
        s.sum[bucket] += cb;
        s.top.add(Chunk{cb, SIZE_T(e.lpData), 0, 0});
    }
    HeapUnlock(h);
    return s;
}

void report_heap(HANDLE h, int idx, HeapStat *s) {
    std::printf("[mem] 堆#%d @0x%llX 存活块=%zu 合计=%.2f MiB%s\n", idx,
                unsigned long long(SIZE_T(h)), s->blocks, mb(s->bytes),
                s->walkable ? "" : "（不可枚举）");
    if (!s->walkable) return;
    for (size_t i = 0; i < kBucketNames.size(); ++i) {
        if (s->count[i] == 0) continue;
        std::printf("[mem]   %-9s x%-9zu %9.2f MiB\n", kBucketNames[i], s->count[i], mb(s->sum[i]));
    }
    top_line("最大存活块", s->top, 2);
}

} // namespace

namespace chrome {

void dump_mem_snapshot(const char *tag) {
    VmTotals t;
    sweep_virtual(&t);
    std::printf("[mem] === %s | Qt=%s 控件=%d ===\n", tag, qVersion(),
                int(QApplication::allWidgets().size()));
    std::printf("[mem] 提交区段 %zu 个：MEM_PRIVATE %.2f MiB / 映射 %.2f MiB / 映像 %.2f MiB\n",
                t.regions, mb(t.priv), mb(t.mapped), mb(t.image));
    if (!t.residency_ok) {
        std::printf("[mem] 私有工作集 n/a（K32QueryWorkingSetEx 不可用或查询失败）\n");
    } else {
        std::printf("[mem] 私有工作集 %.2f MiB：MEM_PRIVATE %.2f / 映射 %.2f / 映像 %.2f\n",
                    mb(t.priv_resident + t.mapped_private_resident + t.image_private_resident),
                    mb(t.priv_resident), mb(t.mapped_private_resident), mb(t.image_private_resident));
    }
    top_line("最大私有区段（按驻留）", t.big_priv, 1);
    top_line("最大映射区段", t.big_map, 0);

    const std::vector<HANDLE> heaps = list_heaps();
    std::printf("[mem] 堆 %zu 个\n", heaps.size());
    SIZE_T heap_bytes = 0;
    for (size_t i = 0; i < heaps.size(); ++i) {
        HeapStat s = sweep_heap(heaps[i]);
        heap_bytes += s.bytes;
        report_heap(heaps[i], int(i), &s);
    }
    std::printf("[mem] 堆存活合计 %.2f MiB；MEM_PRIVATE 提交 %.2f MiB；差 %.2f MiB"
                "（含堆空闲/元数据及堆外分配，非碎片量）\n",
                mb(heap_bytes), mb(t.priv), mb(t.priv > heap_bytes ? t.priv - heap_bytes : 0));
    std::fflush(stdout);
}

} // namespace chrome
#else
namespace chrome {
void dump_mem_snapshot(const char *tag) {
    std::printf("[mem] %s: Windows virtual-memory/heap probe unavailable on this platform\n", tag);
    std::fflush(stdout);
}
} // namespace chrome
#endif
