#pragma once
#include "cpp/chrome/ui_theme.h"
#include "cpp/i18n.h"
#include <QCheckBox>
#include <QComboBox>
#include <QEvent>
#include <QFormLayout>
#include <QLabel>
#include <QPainter>
#include <QSpinBox>
#include <QToolButton>

namespace chrome {
inline void paint_field_arrow(QPainter &p, int x, int y, bool up, bool enabled) {
    p.setPen(Qt::NoPen);
    p.setBrush(enabled ? ui_colors().text_muted : ui_colors().text_disabled);
    p.drawPolygon(QPolygon({QPoint(x - 3, y + (up ? 2 : -2)),
                            QPoint(x + 3, y + (up ? 2 : -2)),
                            QPoint(x, y + (up ? -2 : 2))}));
}
class SessionCombo final : public QComboBox {
public:
    explicit SessionCombo(QWidget *parent) : QComboBox(parent) { setFixedHeight(22); }
protected:
    void paintEvent(QPaintEvent *event) override {
        QComboBox::paintEvent(event);
        QPainter p(this);
        paint_field_arrow(p, width() - 9, height() / 2, false, isEnabled());
    }
};
class SessionSpin final : public QSpinBox {
public:
    explicit SessionSpin(QWidget *parent) : QSpinBox(parent) { setFixedHeight(22); }
protected:
    void paintEvent(QPaintEvent *event) override {
        QSpinBox::paintEvent(event);
        QPainter p(this);
        paint_field_arrow(p, width() - 9, 6, true, isEnabled());
        paint_field_arrow(p, width() - 9, height() - 6, false, isEnabled());
    }
};
// A real action within the combo's trailing area, before its disclosure arrow.
class SessionFieldButton final : public QToolButton {
public:
    explicit SessionFieldButton(QWidget *parent) : QToolButton(parent) {
        setFixedSize(20, 22);
        parent->installEventFilter(this);
        place();
    }
protected:
    bool eventFilter(QObject *, QEvent *event) override {
        if (event->type() == QEvent::Resize) place();
        return false;
    }
private:
    void place() { move(parentWidget()->width() - 39, (parentWidget()->height() - height()) / 2); }
};
inline void prepare_session_fields(QFormLayout *form) {
    form->setContentsMargins(0, 0, 0, 0);
    form->setHorizontalSpacing(8);
    form->setVerticalSpacing(6);
    form->setLabelAlignment(Qt::AlignRight | Qt::AlignVCenter);
    form->setFieldGrowthPolicy(QFormLayout::AllNonFixedFieldsGrow);
    int label_width = 62;
    for (int row = 0; row < form->rowCount(); ++row) {
        if (auto *item = form->itemAt(row, QFormLayout::LabelRole)) {
            if (auto *label = dynamic_cast<QLabel *>(item->widget())) {
                const auto metrics = label->fontMetrics();
                for (const auto &text : {label->text(), i18n::tr("字符集:(C)"), i18n::tr("描述:(D)")})
                    label_width = qMax(label_width, metrics.horizontalAdvance(text) + 2);
            }
        }
    }
    for (int row = 0; row < form->rowCount(); ++row) {
        if (auto *item = form->itemAt(row, QFormLayout::LabelRole)) {
            if (auto *label = dynamic_cast<QLabel *>(item->widget())) {
                label->setFixedWidth(label_width);
                label->setAlignment(Qt::AlignRight | Qt::AlignVCenter);
                if (auto *field = form->itemAt(row, QFormLayout::FieldRole))
                    label->setBuddy(field->widget());
            }
        }
    }
}

// Native checkbox semantics retain Space/Tab and accessibility; only the indicator changes.
class SessionSwitch final : public QCheckBox {
public:
    explicit SessionSwitch(QWidget *parent) : QCheckBox(parent) {
        setFixedSize(32, 20);
        setCursor(Qt::PointingHandCursor);
    }
protected:
    bool hitButton(const QPoint &point) const override { return rect().contains(point); }
    void paintEvent(QPaintEvent *) override {
        const auto &colors = ui_colors();
        QPainter p(this);
        p.setRenderHint(QPainter::Antialiasing);
        p.setOpacity(isEnabled() ? 1.0 : 0.4);
        p.setPen(Qt::NoPen);
        p.setBrush(isChecked() ? colors.accent : colors.bg_hover);
        p.drawRoundedRect(QRectF(0, 2, 32, 16), 8, 8);
        p.setBrush(Qt::white);
        p.drawEllipse(QRectF(isChecked() ? 18 : 2, 4, 12, 12));
        if (hasFocus()) {
            p.setBrush(Qt::NoBrush);
            p.setPen(colors.text_secondary);
            p.drawRoundedRect(QRectF(0.5, 0.5, 31, 19), 9, 9);
        }
    }
};
}
