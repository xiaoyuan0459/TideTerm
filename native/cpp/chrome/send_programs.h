#pragma once
#include "cpp/bridge_shim.h"
#include <QObject>
#include <QString>
#include <functional>
class QHBoxLayout;
class QToolButton;
class QWidget;
namespace chrome {
class ProgramSettings final : public QObject {
public:
    ProgramSettings(QWidget *owner, QHBoxLayout *row, std::function<uint32_t()> source,
                    std::function<void()> invalidate, std::function<void()> refresh,
                    std::function<QString(uint8_t)> explain);
    QToolButton *button() const { return button_; }
    bool enabled() const;
    const ProgramSource &settings() const { return settings_; }
    void sync(ProgramSource settings, bool enabled);
    EnvironmentBatch request(uint64_t owner, const CompletionContext &context, rust::Str query, uint8_t dialect) const;
private:
    bool edit();
    QWidget *owner_;
    QToolButton *button_;
    ProgramSource settings_;
    std::function<uint32_t()> source_;
    std::function<void()> invalidate_, refresh_;
    std::function<QString(uint8_t)> explain_;
};
}
