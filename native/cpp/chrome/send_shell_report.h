#pragma once
#include <cstdint>
#include <functional>
class QHBoxLayout;
class QLineEdit;
class QWidget;
namespace chrome {
void install_shell_report(QWidget *owner, QHBoxLayout *row, std::function<uint32_t()> source,
                          std::function<void()> invalidate, std::function<bool()> enabled, QLineEdit *input);
}
