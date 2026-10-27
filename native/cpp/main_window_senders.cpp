// Sender drafts and selections belong to the owning view/window.
#include "cpp/main_window.h"
#include "cpp/bridge_shim.h"
#include "cpp/chrome/multiline_dialog.h"
#include "cpp/chrome/send_bar.h"
#include "cpp/terminal/terminal_view.h"
#include <QDialog>
#include <QDialogButtonBox>
#include <QLabel>
#include <QListWidget>
#include <QVBoxLayout>
#include <algorithm>
#include <memory>

namespace {
QString q(const rust::String &value) { return QString::fromUtf8(value.data(), int(value.size())); }
using View = QPointer<PtyView>;
struct Target { View view; QString uid; };
}

void MainWindow::configure_sendbar(SendBar *bar, std::function<PtyView *()> source) {
    auto selected = std::make_shared<std::vector<View>>();
    bar->set_target_picker([this, selected] {
        QDialog dialog(this);
        dialog.setObjectName(QStringLiteral("senderTargets"));
        dialog.setWindowTitle(i18n::tr("选择发送目标"));
        dialog.resize(560, 360);
        auto *layout = new QVBoxLayout(&dialog);
        auto *hint = new QLabel(i18n::tr("只向勾选的本窗口会话发送；阅读或锁定时不写入"), &dialog);
        hint->setWordWrap(true); layout->addWidget(hint);
        auto *list = new QListWidget(&dialog);
        list->setObjectName(QStringLiteral("senderTargetList"));
        layout->addWidget(list);
        std::vector<View> choices;
        for (auto *view : views(this)) {
            if (view->is_closed()) continue;
            choices.push_back(view);
            auto *item = new QListWidgetItem(i18n::tr("%1 · 会话 %2").arg(q_title(view->tab())).arg(choices.size()), list);
            item->setData(Qt::UserRole, view->tab()); item->setToolTip(q(term_session_url(view->tab())));
            item->setFlags(item->flags() | Qt::ItemIsUserCheckable);
            item->setCheckState(std::find(selected->begin(), selected->end(), View(view)) != selected->end()
                ? Qt::Checked : Qt::Unchecked);
        }
        auto *buttons = new QDialogButtonBox(QDialogButtonBox::Ok | QDialogButtonBox::Cancel, &dialog);
        connect(buttons, &QDialogButtonBox::accepted, &dialog, &QDialog::accept);
        connect(buttons, &QDialogButtonBox::rejected, &dialog, &QDialog::reject);
        layout->addWidget(buttons);
        if (dialog.exec() != QDialog::Accepted) return -1;
        selected->clear();
        for (int row = 0; row < list->count(); ++row) {
            const auto view = choices[size_t(row)];
            if (list->item(row)->checkState() == Qt::Checked && view && !view->is_closed()) selected->push_back(view);
        }
        return int(selected->size());
    });
    bar->set_writer_factory([this, source, selected](int scope) -> SendBar::Writer {
        auto *origin = source();
        if (!origin || origin->is_closed()) return {};
        std::vector<View> chosen;
        if (scope == 3) chosen = *selected;
        else {
            const auto ids = send_target_ids(origin->tab(), scope);
            for (auto *view : views(this))
                if (!view->is_closed() && std::find(ids.begin(), ids.end(), view->tab()) != ids.end()) chosen.push_back(view);
        }
        std::vector<Target> targets;
        for (auto view : chosen) {
            if (!view || view->is_closed()) return {};
            targets.push_back({view, q(term_uid(view->tab()))});
        }
        if (targets.empty()) return {};
        const Target context{View(origin), q(term_uid(origin->tab()))};
        return [targets, context](const std::string &bytes, bool raw) {
            if (!context.view || context.view->is_closed() || q(term_uid(context.view->tab())) != context.uid) return false;
            std::vector<uint32_t> ids;
            for (const auto &target : targets) {
                if (!target.view || target.view->is_closed() || q(term_uid(target.view->tab())) != target.uid) return false;
                ids.push_back(target.view->tab());
            }
            return term_write_sessions(rust::Slice<const uint32_t>(ids.data(), ids.size()),
                rust::Slice<const uint8_t>(reinterpret_cast<const uint8_t *>(bytes.data()), bytes.size()), raw) == ids.size();
        };
    }, [this, bar](const QString &message) {
        note(message);
        if (auto *status = bar->window()->findChild<QLabel *>(QStringLiteral("senderStatus"))) status->setText(message);
    });
    bar->set_completion_source([source] {
        const auto *view = source();
        return view && !view->is_closed() ? view->tab() : UINT32_MAX;
    });
    bar->set_multiline_opener([this, source, selected, bar](const QString &text, int scope) {
        std::vector<uint32_t> ids;
        if (scope == 3) for (auto view : *selected)
            if (view && !view->is_closed()) ids.push_back(view->tab());
        chrome::show_multiline(this, source(), text, 0, scope, ids, bar->window() == this ? nullptr : bar);
    });
}

void MainWindow::new_sender_window() {
    auto *view = active_view();
    if (!view || view->is_closed() || !palette_allowed()) { note(i18n::tr("请先打开可用会话")); return; }
    if (findChildren<QDialog *>(QStringLiteral("senderWindow")).size() >= 8) {
        note(i18n::tr("每个窗口最多 8 个独立发送窗口")); return;
    }
    auto *dialog = new QDialog(this, Qt::Window);
    dialog->setObjectName(QStringLiteral("senderWindow"));
    dialog->setAttribute(Qt::WA_DeleteOnClose);
    const int sequence = property("senderSequence").toInt() + 1;
    setProperty("senderSequence", sequence);
    dialog->setWindowTitle(i18n::tr("发送 %1 — %2").arg(sequence).arg(q_title(view->tab())));
    dialog->setProperty("sourceTab", view->tab());
    dialog->resize(1040, 220);
    auto *layout = new QVBoxLayout(dialog);
    auto *bar = new SendBar(dialog);
    const View fixed(view);
    configure_sendbar(bar, [fixed] { return fixed.data(); });
    connect(dialog, &QDialog::finished, bar, [bar] { bar->cancel(); });
    layout->addWidget(bar, 1);
    auto *hint = new QLabel(i18n::tr("关闭此窗口会停止发送并丢弃本窗口草稿"), dialog);
    hint->setWordWrap(true); layout->addWidget(hint);
    auto *status = new QLabel(dialog);
    status->setObjectName(QStringLiteral("senderStatus"));
    status->setTextFormat(Qt::PlainText); status->setWordWrap(true); layout->addWidget(status);
    dialog->show(); bar->focus_input();
}
