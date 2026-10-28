#pragma once

// Canonical menu item values and the terminal edit action group.
namespace chrome::menu_data {
// ---- 表结构（数据驱动；条目 flag 位区分 分隔/可勾/禁用/默认勾选/子菜单头） ----
enum : unsigned char {
    kNone = 0,
    kSep = 1,   // 分隔符（其余字段无效）
    kCheck = 2, // 可勾选
    kDis = 4,   // 禁用
    kOn = 8,    // 默认勾选（需 | kCheck）
    kSub = 16,  // 子菜单头（children 生效）
};

struct Item {
    const char *id;
    const char *label;
    const char *shortcut; // canonical 快捷键文本；"" = 无
    unsigned char flags;
    const Item *children;
    int n_children;
};

constexpr Item kEdit[] = {
    {"edit:clear-scrollback", "清除回滚", "Ctrl+Shift+L", kNone, nullptr, 0},
    {"edit:clear-screen", "清屏", "", kNone, nullptr, 0},
    {"edit:clear-all", "全部清除", "", kNone, nullptr, 0},
    {nullptr, nullptr, nullptr, kSep, nullptr, 0},
    {"edit:copy", "复制", "Shift+Del", kNone, nullptr, 0},
    {"edit:paste", "粘贴", "Shift+Ins", kNone, nullptr, 0},
    {"edit:paste-selected", "粘贴选定的文本", "Ctrl+Shift+X", kNone, nullptr, 0},
    {nullptr, nullptr, nullptr, kSep, nullptr, 0},
    {"edit:copy-session-name", "复制会话名称", "", kNone, nullptr, 0},
    {"edit:copy-session-url", "复制会话 URL", "", kNone, nullptr, 0},
    {"completion:settings", "命令建议来源设置…", "", kNone, nullptr, 0},
    {"completion:refresh", "采集 Shell 名字与资源…", "", kNone, nullptr, 0},
};
} // namespace chrome::menu_data
