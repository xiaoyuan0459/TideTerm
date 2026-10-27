#pragma once

#include "cpp/chrome/icon_cache.h"
#include <QLabel>
#include <QPainter>

namespace chrome {

// QLabel chrome glyphs follow the destination window's DPI and current theme at paint time.
class IconLabel final : public QLabel {
public:
    explicit IconLabel(QWidget *parent = nullptr) : QLabel(parent) {}
    void set_glyph(const QString &name, int size) {
        if (name_ == name && size_ == size) return;
        name_ = name;
        size_ = size;
        updateGeometry();
        update();
    }
    QSize sizeHint() const override {
        return name_.isEmpty() ? QLabel::sizeHint() : QSize(size_, size_);
    }
    void set_ink(const QColor &ink) { if (ink_ != ink) { ink_ = ink; update(); } }
protected:
    void paintEvent(QPaintEvent *event) override {
        QLabel::paintEvent(event);
        if (name_.isEmpty()) return;
        QPainter painter(this);
        scalable_icon(name_, size_, ink_).paint(&painter, contentsRect(), Qt::AlignCenter);
    }
private:
    QString name_;
    int size_ = 0;
    QColor ink_;
};

} // namespace chrome
