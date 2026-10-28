#include "cpp/chrome/app_icon.h"

#include <QApplication>
#include <QCoreApplication>
#include <QFileInfo>
#include <QIcon>
#ifdef Q_OS_WIN
#include <QIconEngine>
#include <QImageReader>
#include <QPainter>
#include <QPixmap>
#include <utility>
#endif

namespace chrome {

#ifdef Q_OS_WIN
namespace {
// This shipped ICO has one image per size. QIcon(path) eagerly decodes every ICO entry.
class AppIconEngine final : public QIconEngine {
public:
    explicit AppIconEngine(QString path) : path_(std::move(path)) {
        QImageReader reader(path_, "ico");
        for (int i = 0; i < reader.imageCount(); ++i) {
            if (reader.jumpToImage(i) && reader.size().isValid()) {
                entries_.append({i, reader.size(), {}});
            }
        }
    }

    QIconEngine *clone() const override { return new AppIconEngine(*this); }
    bool isNull() override { return entries_.isEmpty(); }
    QList<QSize> availableSizes(QIcon::Mode mode, QIcon::State state) override {
        QList<QSize> sizes;
        if (mode == QIcon::Normal && state == QIcon::Off) {
            for (const auto &entry : entries_) sizes.append(entry.size);
        }
        return sizes;
    }
    QSize actualSize(const QSize &size, QIcon::Mode, QIcon::State) override {
        const int index = best_entry(size);
        return index < 0 ? QSize() : entries_[index].size.scaled(size, Qt::KeepAspectRatio)
                                                   .boundedTo(entries_[index].size);
    }
    QPixmap pixmap(const QSize &size, QIcon::Mode mode, QIcon::State state) override {
        return scaledPixmap(size, mode, state, 1.0);
    }
    QPixmap scaledPixmap(const QSize &size, QIcon::Mode mode, QIcon::State state,
                         qreal scale) override {
        const int index = best_entry(size * scale);
        return index < 0 ? QPixmap() : load(index).pixmap(size, scale, mode, state);
    }
    void paint(QPainter *painter, const QRect &rect, QIcon::Mode mode,
               QIcon::State state) override {
        const qreal scale = painter->device()->devicePixelRatioF();
        painter->drawPixmap(rect, scaledPixmap(rect.size(), mode, state, scale));
    }

private:
    struct Entry { int image_index; QSize size; QIcon icon; };
    static qint64 area(const QSize &size) { return qint64(size.width()) * size.height(); }
    int best_entry(const QSize &size) const {
        int best = -1;
        const qint64 wanted = area(size);
        for (int i = 0; i < entries_.size(); ++i) {
            if (best < 0) { best = i; continue; }
            const qint64 current = area(entries_[best].size);
            const qint64 candidate = area(entries_[i].size);
            if (current >= wanted ? candidate >= wanted && candidate < current
                                  : candidate > current) best = i;
        }
        return best;
    }
    QIcon &load(int index) {
        auto &entry = entries_[index];
        if (entry.icon.isNull()) {
            QImageReader reader(path_, "ico");
            if (reader.jumpToImage(entry.image_index)) {
                entry.icon = QIcon(QPixmap::fromImage(reader.read()));
            }
        }
        return entry.icon;
    }
    QString path_;
    QList<Entry> entries_;
};
} // namespace
#endif

void install_app_icon(QApplication &app) {
    const QString exe_dir = QCoreApplication::applicationDirPath();
#ifdef Q_OS_WIN
    const QString name = QStringLiteral("tideterm-icon.ico");
#else
    const QString name = QStringLiteral("tideterm-icon.png");
#endif
    const QString shipped = exe_dir + QLatin1Char('/') + name;
    const QString development = exe_dir + QStringLiteral("/../../assets/icons/") + name;
    const QString path = QFileInfo::exists(shipped) ? shipped : development;
    if (QFileInfo::exists(path)) {
#ifdef Q_OS_WIN
        app.setWindowIcon(QIcon(new AppIconEngine(path)));
#else
        app.setWindowIcon(QIcon(path));
#endif
    }
}

} // namespace chrome
