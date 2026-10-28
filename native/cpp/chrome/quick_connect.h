// Address-only quick connect. SSH opens the compact login, not the new-session form.
#pragma once

#include <cstdint>
#include <QString>

class QWidget;

/// 运行快速连接对话框。返回新标签 id（u32::MAX = 取消/失败）。
uint32_t run_quick_connect(QWidget *parent, const QString &address = QString());
