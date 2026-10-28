//! `BudgetPool` / `DocumentManager` —— 256 MiB 全局共池仲裁（D1a）。
//!
//! **核心不变量（用类型系统保证，非评审纪律）**：[`EvictTarget`] 的候选集里
//! **没有「终端已提交滚动历史行」这一类** —— 终端侧唯一的驱逐目标是
//! `TerminalExpandLru`（展开缓存）。PTY 输出已被消费、历史不可重建，因此
//! 历史行**在任何预算压力下都不会被本池丢弃**（G4-13/15 的机械保证）。
//!
//! 驱逐顺序（ADR-013 §Decision 6 的**硬规则**，不是启发式）：
//! `日志冷层 → 日志温层 → 日志解码缓存 → LineIndex → 终端展开 LRU`。
//! 同层内按文档 LRU（最久未用先逐出）。
//!
//! 终端类超上限：**保留数据 + 返回报警状态**（[`TerminalStatus::OverLimit`]），
//! 允许短暂突破 256 MiB —— 宁可超预算也不静默丢用户数据（G4-15）。

use std::collections::HashMap;

use crate::core::RowProvider;
use crate::provider::LogDocument;

/// 全局池硬上限（D1 已确认：256 MiB）。
pub const DEFAULT_POOL_BUDGET: usize = 256 << 20;
/// 终端单窗格预算（与今日 `scrollback_mb` 默认 32 MiB 一致）。
pub const TERMINAL_PANE_BUDGET: usize = 32 << 20;

/// 驱逐目标。**没有** `TerminalHistory` 变体 —— 这就是 G4-13 的类型级保证。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvictTarget {
    /// 日志冷层块（文件类恒 0：冷层在磁盘；枚举位保留以对齐 ADR 驱逐顺序）。
    LogCold,
    /// 日志温层行（文件类恒 0；终端温层在 ScrollbackStore 且不可驱逐）。
    LogWarm,
    /// 日志解码缓存（ChunkCache）。
    LogDecode,
    /// 稀疏行索引采样表（重扫可重建）。
    LogIndex,
    /// 终端侧唯一可驱逐项：展开行 LRU（已提交历史不受影响）。
    TerminalExpandLru,
}

/// 池用量快照（可观测性 + G4-10/G4-5 口径）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PoolUsage {
    pub terminal: usize,
    pub log_cold: usize,
    pub log_warm: usize,
    pub log_decode: usize,
    pub log_index: usize,
}

impl PoolUsage {
    pub fn total(&self) -> usize {
        self.terminal + self.log_cold + self.log_warm + self.log_decode + self.log_index
    }
    pub fn log_total(&self) -> usize {
        self.log_cold + self.log_warm + self.log_decode + self.log_index
    }
}

/// 终端用量上报结果（G4-15 行为门禁）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalStatus {
    Ok,
    /// 超上限：**数据保留**，宿主必须呈现报警（返回值即报警信号）。
    OverLimit {
        used: usize,
        cap: usize,
    },
}

#[derive(Default)]
struct DocAccount {
    cold: usize,
    warm: usize,
    decode: usize,
    index: usize,
}

impl DocAccount {
    fn by_target(&self, t: EvictTarget) -> usize {
        match t {
            EvictTarget::LogCold => self.cold,
            EvictTarget::LogWarm => self.warm,
            EvictTarget::LogDecode => self.decode,
            EvictTarget::LogIndex => self.index,
            EvictTarget::TerminalExpandLru => 0,
        }
    }
}

/// 256 MiB 共池。记账 + 裁决（`next_victim`）；实际驱逐由 [`DocumentManager`] 执行。
pub struct BudgetPool {
    budget: usize,
    terminal_panes: usize,
    terminal_used: usize,
    terminal_expand_lru: usize,
    /// 日志文档 LRU：队尾 = 最近使用。
    lru: Vec<u64>,
    accounts: HashMap<u64, DocAccount>,
}

impl BudgetPool {
    pub fn new(budget: usize) -> Self {
        BudgetPool {
            budget,
            terminal_panes: 1,
            terminal_used: 0,
            terminal_expand_lru: 0,
            lru: Vec::new(),
            accounts: HashMap::new(),
        }
    }

    pub fn budget(&self) -> usize {
        self.budget
    }

    /// 终端类上限：`min(pane 数 × 32 MiB, 池的 50%)`（D1a：1/4 pane 与今日一致，
    /// 8 pane 时每 pane 降为 16 MiB —— 共池的唯一行为变化）。
    pub fn set_terminal_panes(&mut self, panes: usize) {
        self.terminal_panes = panes.max(1);
    }

    pub fn terminal_cap(&self) -> usize {
        (self.terminal_panes * TERMINAL_PANE_BUDGET).min(self.budget / 2)
    }

    /// 终端用量上报（widget 侧 `estimated_bytes()`）。超上限**不驱逐**，
    /// 返回报警状态（G4-15）。
    pub fn report_terminal(&mut self, used: usize, expand_lru: usize) -> TerminalStatus {
        self.terminal_used = used;
        self.terminal_expand_lru = expand_lru;
        let cap = self.terminal_cap();
        if used > cap {
            TerminalStatus::OverLimit { used, cap }
        } else {
            TerminalStatus::Ok
        }
    }

    pub fn register_doc(&mut self, id: u64) {
        if !self.lru.contains(&id) {
            self.lru.push(id);
            self.accounts.entry(id).or_default();
        }
    }

    pub fn close_doc(&mut self, id: u64) {
        self.lru.retain(|&x| x != id);
        self.accounts.remove(&id);
    }

    pub fn touch_doc(&mut self, id: u64) {
        self.lru.retain(|&x| x != id);
        self.lru.push(id);
    }

    /// 刷新某文档的分层记账（provider 上报）。
    pub fn report_doc(&mut self, id: u64, cold: usize, warm: usize, decode: usize, index: usize) {
        let a = self.accounts.entry(id).or_default();
        a.cold = cold;
        a.warm = warm;
        a.decode = decode;
        a.index = index;
    }

    pub fn usage(&self) -> PoolUsage {
        let mut u = PoolUsage {
            terminal: self.terminal_used,
            ..PoolUsage::default()
        };
        for a in self.accounts.values() {
            u.log_cold += a.cold;
            u.log_warm += a.warm;
            u.log_decode += a.decode;
            u.log_index += a.index;
        }
        u
    }

    /// 裁决下一个驱逐目标（硬规则顺序 + 文档 LRU）。返回 `(doc_id, target)`；
    /// 全部日志层已空时给出终端展开 LRU；再无候选返回 `None`（此后若仍超预算，
    /// 只能是终端历史 —— 按超限语义保留并报警）。
    pub fn next_victim(&self) -> Option<(u64, EvictTarget)> {
        // 同层内：LRU 队首（最久未用）优先。
        const ORDER: [EvictTarget; 4] = [
            EvictTarget::LogCold,
            EvictTarget::LogWarm,
            EvictTarget::LogDecode,
            EvictTarget::LogIndex,
        ];
        for target in ORDER {
            for &id in &self.lru {
                if let Some(a) = self.accounts.get(&id)
                    && a.by_target(target) > 0
                {
                    return Some((id, target));
                }
            }
        }
        None
    }

    pub fn has_terminal_expand_lru(&self) -> bool {
        self.terminal_expand_lru > 0
    }
}

/// 文档管理器：池 + 文档集合 + 驱逐执行 + 驱逐审计（G4-14 需要驱逐明细）。
pub struct DocumentManager {
    pool: BudgetPool,
    docs: HashMap<u64, LogDocument>,
    next_id: u64,
    /// 驱逐审计：`(doc, target, freed_bytes)`。
    evictions: Vec<(u64, EvictTarget, usize)>,
}

impl DocumentManager {
    pub fn new(budget: usize) -> Self {
        DocumentManager {
            pool: BudgetPool::new(budget),
            docs: HashMap::new(),
            next_id: 1,
            evictions: Vec::new(),
        }
    }

    pub fn pool(&mut self) -> &mut BudgetPool {
        &mut self.pool
    }

    /// 打开日志文档（惰性索引）。返回句柄 id。
    pub fn open(&mut self, path: impl AsRef<std::path::Path>) -> std::io::Result<u64> {
        let doc = LogDocument::open(path)?;
        let id = self.next_id;
        self.next_id += 1;
        self.docs.insert(id, doc);
        self.pool.register_doc(id);
        self.pool.touch_doc(id);
        self.refresh_account(id);
        Ok(id)
    }

    pub fn close(&mut self, id: u64) -> bool {
        self.pool.close_doc(id);
        self.docs.remove(&id).is_some()
    }

    pub fn get(&self, id: u64) -> Option<&LogDocument> {
        self.docs.get(&id)
    }

    pub fn get_mut(&mut self, id: u64) -> Option<&mut LogDocument> {
        self.docs.get_mut(&id)
    }

    /// 读行（消费即 touch —— LRU 语义）。执行后跑一轮预算仲裁。
    /// 返回归一 UTF-8 的行内容（一致性口径；C++ 侧经 cxx 桥拿的也是它）。
    pub fn row(&mut self, id: u64, i: usize) -> Option<String> {
        let bytes = {
            let r = self.docs.get_mut(&id)?.row(i)?;
            let mut b = Vec::new();
            r.write_utf8(&mut b);
            b
        };
        self.pool.touch_doc(id);
        self.refresh_account(id);
        self.enforce();
        Some(String::from_utf8_lossy_owned(bytes))
    }

    /// 后台索引驱动（宿主定时调用）：全部文档各推进一步。
    pub fn index_step(&mut self, max_bytes_each: u64) {
        let ids: Vec<u64> = self.docs.keys().copied().collect();
        for id in ids {
            if let Some(doc) = self.docs.get_mut(&id) {
                doc.index_more(max_bytes_each);
            }
            self.pool.touch_doc(id);
            self.refresh_account(id);
        }
        self.enforce();
    }

    fn refresh_account(&mut self, id: u64) {
        if let Some(doc) = self.docs.get(&id) {
            let (decode, index) = doc.ram_bytes();
            self.pool.report_doc(id, 0, 0, decode, index);
        }
    }

    /// 预算仲裁循环：按硬规则逐出直至总量 ≤ 预算或无候选。
    /// 返回本轮驱逐数。
    pub fn enforce(&mut self) -> usize {
        let mut n = 0;
        while self.pool.usage().total() > self.pool.budget() {
            let Some((id, target)) = self.pool.next_victim() else {
                break; // 日志层全空；剩余超预算部分是终端类 → 超限语义（G4-15）
            };
            let freed = match target {
                EvictTarget::LogDecode => self.docs.get_mut(&id).map_or(0, |d| d.evict_decode()),
                EvictTarget::LogIndex => self.docs.get_mut(&id).map_or(0, |d| d.evict_index()),
                // 文件类恒 0 的两个层：占位（终端温层在 ScrollbackStore，不可驱逐）。
                EvictTarget::LogCold | EvictTarget::LogWarm => 0,
                EvictTarget::TerminalExpandLru => 0, // 由 widget 回调执行（POC 未接）
            };
            self.refresh_account(id);
            self.evictions.push((id, target, freed));
            n += 1;
            if freed == 0 && matches!(target, EvictTarget::LogCold | EvictTarget::LogWarm) {
                // 恒 0 层不会推进循环 —— 跳过防死循环。
                continue;
            }
            if freed == 0 {
                break; // 无进展（防御）
            }
        }
        n
    }

    pub fn evictions(&self) -> &[(u64, EvictTarget, usize)] {
        &self.evictions
    }

    pub fn usage(&self) -> PoolUsage {
        self.pool.usage()
    }

    /// 终端用量上报透传（widget 每帧调用）。
    pub fn report_terminal(&mut self, used: usize, expand_lru: usize) -> TerminalStatus {
        self.pool.report_terminal(used, expand_lru)
    }
}
