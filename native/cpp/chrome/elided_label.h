// Keep the full notification available to accessibility and tooltips while painting one line.
#pragma once

#include <QHelpEvent>
#include <QLabel>
#include <QPainter>
#include <QStyle>
#include <QToolTip>

namespace chrome {

class ElidedLabel : public QLabel {
public:
    explicit ElidedLabel(QWidget *parent = nullptr) : QLabel(parent) {
        setSizePolicy(QSizePolicy::Preferred, QSizePolicy::Fixed);
        setMinimumWidth(0);
    }

    QSize minimumSizeHint() const override { return {0, QLabel::minimumSizeHint().height()}; }

protected:
    void paintEvent(QPaintEvent *) override {
        QPainter painter(this);
        const QString visible = fontMetrics().elidedText(text(), Qt::ElideRight, contentsRect().width());
        style()->drawItemText(&painter, contentsRect(), alignment() | Qt::TextSingleLine,
                              palette(), isEnabled(), visible, foregroundRole());
    }

    bool event(QEvent *event) override {
        if (event->type() == QEvent::ToolTip && fontMetrics().horizontalAdvance(text()) > contentsRect().width()) {
            auto *help = static_cast<QHelpEvent *>(event);
            QToolTip::showText(help->globalPos(), text(), this);
            return true;
        }
        return QLabel::event(event);
    }
};

} // namespace chrome
