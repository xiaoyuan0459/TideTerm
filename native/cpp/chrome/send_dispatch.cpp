// Byte encoding and cancellable send scheduling are independent of toolbar layout.
#include "cpp/chrome/send_bar.h"
#include "cpp/i18n.h"
#include <QComboBox>
#include <QDoubleSpinBox>
#include <QLineEdit>
#include <QTextBoundaryFinder>
#include <QTimer>

namespace {
int hex_val(QChar c) {
    const auto u = c.unicode();
    if (u >= u'0' && u <= u'9') return int(u - u'0');
    if (u >= u'a' && u <= u'f') return int(u - u'a') + 10;
    if (u >= u'A' && u <= u'F') return int(u - u'A') + 10;
    return -1;
}
}

bool SendBar::build_bytes(std::string *out) const {
    std::string bytes;
    std::vector<QLineEdit *> fields = {input_};
    fields.insert(fields.end(), segments_.begin(), segments_.end());
    for (size_t part = 0; part < fields.size(); ++part) {
        const QString text = fields[part]->text();
        if (part > 0 && text.isEmpty()) continue;
        if (mode_ == 1) {
            QString hex = text;
            hex.remove(QLatin1Char(' '));
            if (hex.size() % 2 != 0) return false;
            for (int i = 0; i + 1 < hex.size(); i += 2) {
                const int hi = hex_val(hex[i]);
                const int lo = hex_val(hex[i + 1]);
                if (hi < 0 || lo < 0) return false;
                bytes.push_back(char((hi << 4) | lo));
            }
        } else bytes += text.toUtf8().toStdString();
        switch (newline_) {
            case 0: bytes += "\r"; break;
            case 1: bytes += "\n"; break;
            case 2: bytes += "\r\n"; break;
            default: break;
        }
    }
    *out = bytes;
    return true;
}

void SendBar::send() {
    if (timer_->isActive() || pacer_->isActive()) return;
    std::string bytes;
    if (!build_bytes(&bytes)) {
        if (note_) note_(i18n::tr("Hex 输入无效：需要偶数个十六进制字符"));
        return;
    }
    pending_writer_ = writer_factory_ ? writer_factory_(target_->currentIndex()) : Writer{};
    if (!pending_writer_ || bytes.empty()) {
        if (note_) note_(i18n::tr("没有可用的发送目标"));
        return;
    }
    pending_target_ = target_->currentText();
    pending_raw_ = mode_ == 1;
    pending_.clear();
    pending_bytes_ = bytes.size();
    if (!character_mode_) pending_.push_back(bytes);
    else if (mode_ == 1) {
        for (char byte : bytes) pending_.emplace_back(1, byte);
    } else {
        const QString text = QString::fromUtf8(bytes.data(), int(bytes.size()));
        QTextBoundaryFinder boundary(QTextBoundaryFinder::Grapheme, text);
        int start = 0;
        for (int end = boundary.toNextBoundary(); end >= 0; end = boundary.toNextBoundary()) {
            pending_.push_back(text.mid(start, end - start).toUtf8().toStdString());
            start = end;
        }
    }
    remaining_ = repeat_count_ <= 0 ? -1 : repeat_count_;
    dispatch_cycle();
}

void SendBar::dispatch_cycle() {
    if (pending_.empty() || !pending_writer_) { stop_send(); return; }
    next_chunk_ = 1;
    if (!deliver(pending_.front())) return;
    if (pending_.size() == 1) finish_cycle();
    else pacer_->start(qMax(1, int(character_interval_->value() * 1000)));
    update_controls();
}

void SendBar::finish_cycle() {
    pacer_->stop();
    ++sent_;
    if (remaining_ > 0) --remaining_;
    if (remaining_ != 0) timer_->start(int(interval_->value() * 1000));
    else pending_.clear();
    update_controls();
    if (note_) note_(i18n::tr("已发送 %1 字节（%2 · 换行 %3 · %4）")
        .arg(uint(pending_bytes_)).arg(mode_ == 1 ? QStringLiteral("Hex") : i18n::tr("文本"))
        .arg(newline_mode()).arg(pending_target_));
}

bool SendBar::deliver(const std::string &bytes) {
    if (pending_writer_ && pending_writer_(bytes, pending_raw_)) return true;
    stop_send();
    if (note_) note_(i18n::tr("目标不可写、已关闭或重新连接，发送已停止"));
    return false;
}

void SendBar::stop_send() {
    timer_->stop();
    pacer_->stop();
    pending_raw_ = mode_ == 1;
    pending_.clear();
    pending_writer_ = {};
    remaining_ = 0;
    update_controls();
}
