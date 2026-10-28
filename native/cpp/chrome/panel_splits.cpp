#include "cpp/chrome/panel_splits.h"

#include <QByteArray>
#include <QList>
#include <QMap>
#include <QObject>
#include <QPointer>
#include <QPainter>
#include <QRect>
#include <QSettings>
#include <QSplitter>
#include <QSplitterHandle>
#include <QString>
#include <QStringList>

#include <algorithm>
#include <set>

#include "cpp/chrome/split_ratio.h"
#include "cpp/chrome/ui_metrics.h"
#include "cpp/chrome/ui_theme.h"

namespace chrome {
namespace {

constexpr char kKeySplits[] = "layout/panelSplits.v1";
constexpr char kFieldSep[] = "\x1f"; // 条目名 / 值 分隔（不出现在类名与状态串里）

/// 下限的可用空间门槛：跨度不足 4×下限（即总量 <40%）时不设 min——
/// 面板只有 160px 宽时 10% 下限会把两个窗格的最小宽度加起来顶破面板本身。
constexpr int kMinSpanFactor = 4;

/// 手柄自绘：5px 矩形里只画居中的 1px 主题线 + 悬停时中央强调色握点。
/// 视觉与 QSS `QSplitter::handle { background: bd1 }` 同线宽，多出来的 4px 只是鼠标热区
/// （面板比中央窗格窄，1px 手柄在 Windows 缩放下几乎抓不住）；静止保持 1px 细线，
/// 不把分割条画成一根 5px 粗柱子。
class PanelSplitHandle : public QSplitterHandle {
public:
    explicit PanelSplitHandle(Qt::Orientation orientation, QSplitter *parent)
        : QSplitterHandle(orientation, parent) {
        setMouseTracking(true);
    }

protected:
    void paintEvent(QPaintEvent *) override {
        const UiColors &c = ui_colors();
        const bool hovered = underMouse();
        QPainter p(this);
        p.setPen(Qt::NoPen);
        if (orientation() == Qt::Horizontal) {
            p.fillRect(QRect(width() / 2, 0, 1, height()), hovered ? c.accent : c.border1);
            if (hovered) {
                p.setBrush(c.accent);
                p.drawRect(QRect(width() / 2, height() / 2 - metrics::kPanelSplitGripSize / 2, 1,
                                 metrics::kPanelSplitGripSize));
            }
            return;
        }
        p.fillRect(QRect(0, height() / 2, width(), 1), hovered ? c.accent : c.border1);
        if (hovered) {
            p.setBrush(c.accent);
            p.drawRect(QRect(width() / 2 - metrics::kPanelSplitGripSize / 2, height() / 2,
                             metrics::kPanelSplitGripSize, 1));
        }
    }
};

/// 面板分割器：唯一的差别是自绘手柄（`QSplitter::createHandle` 虚函数，plain cxx 下无 moc 需求）。
class PanelSplitter : public QSplitter {
public:
    using QSplitter::QSplitter;

protected:
    QSplitterHandle *createHandle() override {
        return new PanelSplitHandle(orientation(), this);
    }
};

/// 两侧最小尺寸 = 可拖区间的下限（10%）。
///
/// 光靠 `setChildrenCollapsible(false)` 挡不住拖到 0：`QSplitterPrivate::moveSplitter()`
/// 自己会把一侧设成 0 并 emit `splitterMoved`，而钳位槽此时看到 `{0, n}`，与「放大态
/// 把兄弟压到 0」无法区分，只能整个跳过（见 split_ratio.cpp 的豁免条款）。
/// 于是把下限写进子件 min：拖拽与程序性回钳都受它约束，且与 10% 比例上限同源。
void apply_child_mins(QSplitter *sp) {
    if (sp->count() != 2) {
        return;
    }
    const bool horizontal = sp->orientation() == Qt::Horizontal;
    const QList<int> sizes = sp->sizes();
    const int total = sizes.value(0) + sizes.value(1);
    const int span = horizontal ? sp->width() : sp->height();
    const int basis = qMax(total, span);
    if (basis <= 0) {
        return;
    }
    const int low = qMax(1, int(double(basis) * metrics::kSplitRatioMin));
    // 面板比中央窗格窄得多：min 取不到就让开（宁可少一层下限，也不许面板被 min 撑破）。
    if (span > 0 && span < low * kMinSpanFactor) {
        return;
    }
    for (int i = 0; i < 2; ++i) {
        QWidget *w = sp->widget(i);
        if (w == nullptr) {
            continue;
        }
        if (horizontal) {
            w->setMinimumWidth(low);
        } else {
            w->setMinimumHeight(low);
        }
    }
}

/// 登记表：只有经 `install_panel_split()` 装过的分割器才参与持久化。
/// 每次登记时顺手剔除已析构的弱指针（自检里分割器建在栈上，出作用域即销毁）。
std::set<QPointer<QSplitter>> &registry() {
    static std::set<QPointer<QSplitter>> table;
    table.erase(nullptr);
    return table;
}

QString node_name(const QObject *object) {
    const QString obj = object->objectName();
    return obj.isEmpty() ? QString::fromLatin1(object->metaObject()->className()) : obj;
}

/// 分割器的稳定身份 = 从分割器往上、到主事件循环对象的链路，逐级拼名
/// （`MainWindow/SftpBrowser/fileSplit`）。用在 QSettings 键上跨进程稳定，且必须**唯一**：
/// 只按类名当键的话，一个窗口里几个匿名 QSplitter 会挤在同一个键上互相覆盖比例。
QString splitter_name(const QSplitter *sp) {
    QStringList path;
    for (const QObject *object = sp; object != nullptr; object = object->parent()) {
        path.prepend(node_name(object));
        if (object->parent() == nullptr) {
            break;
        }
    }
    return path.join(QLatin1Char('/'));
}

/// 每个分割器一个键：`layout/panelSplits.v1/<链路名>`。
/// 值 = `'h'|'v' + <0x1f> + saveState().toHex()`（取向 + 比例一把存）。
///
/// 不用「一个键装全部条目、用换行连接」：QSettings 的 INI 后端会把值里的换行当续行
/// （读回来只剩第一条），注册表后端虽能存多行但两个后端行为不一致；每键一条既躲开
/// 这个坑，也让陈旧清理能按前缀精确删除。
QString split_value(const QSplitter *sp) {
    const char axis = sp->orientation() == Qt::Horizontal ? 'h' : 'v';
    return QLatin1Char(axis) + QLatin1String(kFieldSep) + QString::fromLatin1(sp->saveState().toHex());
}

QSettings layout_settings() {
    return QSettings(QSettings::defaultFormat(), QSettings::UserScope,
                     QStringLiteral("TideTerm"), QStringLiteral("TideTerm"));
}

/// 读出 `layout/panelSplits.v1/` 组里所有比例条目（key = 分割器名，value = 上条的格式）。
/// 每次现构造 QSettings：QSettings 会缓存读到的那份表，实例活得久了会把别处刚写的值
/// 当不存在（自检里踩到过），读写都靠现构造 + 析构时 sync 保持新鲜。
QMap<QString, QString> read_panel_splits() {
    const QSettings s = layout_settings();
    const QString prefix = QLatin1String(kKeySplits) + QLatin1Char('/');
    QMap<QString, QString> out;
    for (const QString &key : s.allKeys()) {
        if (key.startsWith(prefix)) {
            out.insert(key.mid(prefix.size()), s.value(key).toString());
        }
    }
    return out;
}

} // namespace

int panel_split_handle_width() {
    return metrics::kPanelSplitHandleWidth;
}

SplitClick panel_split_states(int total, int click) {
    if (total <= 0) {
        return {0, 0};
    }
    const int upper = qBound(0, click, total);
    return {upper, total - upper};
}

QSplitter *panel_splitter(Qt::Orientation orientation, QWidget *parent) {
    auto *sp = new PanelSplitter(orientation, parent);
    sp->setHandleWidth(panel_split_handle_width());
    return sp;
}

void install_panel_split(QSplitter *sp) {
    if (sp == nullptr) {
        return;
    }
    install_split_ratio(sp); // 比例钳 0.1–0.9（不碰 handleWidth）
    apply_child_mins(sp);
    // 子件增减后再补一次下限（拖拽产生的新窗格在 addWidget 之后才进来）。
    QObject::connect(sp, &QSplitter::splitterMoved, sp, [sp](int, int) { apply_child_mins(sp); });
    registry().insert(sp);
}

void save_panel_splits() {
    QSettings s = layout_settings();
    // 陈旧清理：上一轮写过、这一轮不存在的分割器（改名 / 被删）不能再留死比例。
    // allKeys() 给的是**相对**键（不带 QSettings 作用域前缀），所以前缀就是 kKeySplits 本身，
    // 末尾带 '/'；不用带斜杠的 kKeySplits + "/" 去比（那样永远匹配不上，清理等于空转）。
    const QString prefix = QLatin1String(kKeySplits) + QLatin1Char('/');
    std::set<QString> seen;
    for (const QPointer<QSplitter> &sp : registry()) {
        if (sp == nullptr) {
            continue;
        }
        const QString name = splitter_name(sp);
        seen.insert(name);
        s.setValue(prefix + name, split_value(sp));
    }
    for (const QString &key : s.allKeys()) {
        if (key.startsWith(prefix) && !seen.count(key.mid(prefix.size()))) {
            s.remove(key);
        }
    }
}

void restore_panel_splits() {
    const QMap<QString, QString> saved = read_panel_splits();
    for (const QPointer<QSplitter> &sp : registry()) {
        if (sp == nullptr) {
            continue;
        }
        const auto it = saved.constFind(splitter_name(sp));
        if (it == saved.constEnd()) {
            continue;
        }
        const QStringList parts = it.value().split(QLatin1String(kFieldSep));
        if (parts.size() != 2) {
            continue;
        }
        const char axis = sp->orientation() == Qt::Horizontal ? 'h' : 'v';
        if (parts.at(0) != QLatin1Char(axis)) {
            continue; // 取向已变（文件管理器按面板宽度切竖排/横排），旧比例作废
        }
        sp->restoreState(QByteArray::fromHex(parts.at(1).toLatin1()));
    }
}

} // namespace chrome
