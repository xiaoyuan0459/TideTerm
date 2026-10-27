#include "cpp/chrome/hover_preview.h"
#include "cpp/chrome/ui_theme.h"
#include "cpp/terminal/terminal_view.h"
#include <QApplication>
#include <QFontMetrics>
#include <QKeyEvent>
#include <QScreen>

namespace chrome {
HoverPreview::HoverPreview(QWidget *target, PtyView *source)
    : QLabel(target, Qt::ToolTip | Qt::WindowDoesNotAcceptFocus | Qt::WindowTransparentForInput), target_(target), source_(source) {
    setObjectName(QStringLiteral("hoverPreview"));
    setAttribute(Qt::WA_ShowWithoutActivating);
    setAttribute(Qt::WA_TransparentForMouseEvents);
    setTextFormat(Qt::PlainText);
    setMargin(10);
    setFrameStyle(QFrame::Box | QFrame::Plain);
    setAutoFillBackground(true);
    setFocusPolicy(Qt::NoFocus);
    delay_.setSingleShot(true);
    delay_.setInterval(250);
    connect(&delay_, &QTimer::timeout, this, [this] { display(); });
    qApp->installEventFilter(this);
    hide();
}

HistoryState HoverPreview::state() const {
    return source_ && !source_->is_closed() ? term_history_state(source_->tab()) : HistoryState{};
}

void HoverPreview::request(uint64_t key, QPoint global, std::function<QString()> fetch) {
    const auto current = state();
    if (!target_ || !target_->isVisible() || !current.available) { cancel(); return; }
    if (key_ == key && generation_ == current.generation && (delay_.isActive() || isVisible())) return;
    cancel();
    key_ = key; anchor_ = global; fetch_ = std::move(fetch); generation_ = current.generation;
    delay_.start();
}

void HoverPreview::cancel() {
    delay_.stop(); hide(); clear(); fetch_ = {}; key_ = 0;
}

void HoverPreview::refresh() {
    if (fetch_ && !delay_.isActive()) { hide(); delay_.start(); }
}

void HoverPreview::display() {
    const auto current = state();
    if (!fetch_ || !target_ || !target_->isVisible() || !current.available || current.generation != generation_) {
        cancel(); return;
    }
    const QString value = fetch_();
    if (value.isEmpty()) { cancel(); return; }
    shown_ = current;
    const auto family = term_font_family();
    QFont font(QString::fromUtf8(family.data(), int(family.size())));
    font.setPixelSize(int(term_font_px()));
    setFont(font);
    QPalette colors = palette();
    colors.setColor(QPalette::Window, ui_colors().term_bg);
    colors.setColor(QPalette::WindowText, ui_colors().term_fg);
    setPalette(colors);
    auto *screen = QGuiApplication::screenAt(anchor_);
    const QRect available = (screen ? screen : target_->screen())->availableGeometry().adjusted(12, 12, -12, -12);
    const int content_width = qMax(1, qMin(620, available.width() - 24));
    const QFontMetrics metrics(font);
    const int max_lines = qMax(1, (available.height() - 24) / metrics.lineSpacing());
    const QStringList lines = value.split(QLatin1Char('\n'));
    QStringList visible;
    for (int at = 0; at < qMin(int(lines.size()), max_lines); ++at)
        visible << metrics.elidedText(lines[at], Qt::ElideRight, content_width);
    if (lines.size() > max_lines) visible.last() = QStringLiteral("…");
    setText(visible.join(QLatin1Char('\n')));
    setMaximumWidth(content_width + 24); adjustSize();
    int x = anchor_.x() + 16;
    if (x + width() > available.right()) x = anchor_.x() - width() - 16;
    move(qBound(available.left(), x, qMax(available.left(), available.right() - width())),
         qBound(available.top(), anchor_.y() + 16, qMax(available.top(), available.bottom() - height())));
    show();
}

bool HoverPreview::eventFilter(QObject *watched, QEvent *event) {
    if (!delay_.isActive() && !isVisible()) return false;
    auto *widget = qobject_cast<QWidget *>(watched);
    const bool owner = widget && target_ && widget->window() == target_->window();
    switch (event->type()) {
    case QEvent::ApplicationDeactivate:
        cancel(); break;
    case QEvent::Hide:
    case QEvent::Resize:
    case QEvent::Move:
    case QEvent::WindowDeactivate:
        if (watched == target_ || watched == source_ || (target_ && watched == target_->window())) cancel();
        break;
    case QEvent::Leave:
        if (watched == target_) cancel();
        break;
    case QEvent::KeyPress:
        if (owner) {
            const auto *key = static_cast<QKeyEvent *>(event);
            const bool escape = key->key() == Qt::Key_Escape && key->modifiers() == Qt::NoModifier;
            cancel(); if (escape) { event->accept(); return true; }
        }
        break;
    case QEvent::ShortcutOverride:
        if (owner) {
            const auto *key = static_cast<QKeyEvent *>(event);
            if (key->key() == Qt::Key_Escape && key->modifiers() == Qt::NoModifier) {
                event->accept(); return true; // Keep Escape for the preview's KeyPress handler.
            }
            cancel();
        }
        break;
    case QEvent::MouseButtonPress:
    case QEvent::MouseButtonRelease:
    case QEvent::Wheel:
        if (owner) cancel();
        break;
    case QEvent::Paint:
        if (watched == source_) {
            const auto current = state();
            if (!current.available || current.generation != generation_) cancel();
            else if (isVisible() && (current.revision != shown_.revision || current.maximum != shown_.maximum ||
                                    current.cols != shown_.cols || current.rows != shown_.rows)) {
                hide(); delay_.start(); // Coalesce source paints; no periodic text extraction.
            }
        }
        break;
    default: break;
    }
    return false;
}
}
