#include "cpp/chrome/selftest_settings.h"
// AiPanel 实现（Phase C ⑥）。

#include "cpp/i18n.h"
#include "cpp/chrome/ai_panel.h"

#include "cpp/bridge_shim.h"
#include "cpp/chrome/docks.h"
#include "cpp/main_window.h"
#include "cpp/terminal/terminal_view.h"

#include <QApplication>
#include <QCheckBox>
#include <QComboBox>
#include <QDialog>
#include <QDialogButtonBox>
#include <QDockWidget>
#include <QFormLayout>
#include <QHBoxLayout>
#include <QLabel>
#include <QLineEdit>
#include <QListWidget>
#include <QPixmap>
#include <QPlainTextEdit>
#include <QPushButton>
#include <QTimer>
#include <QVBoxLayout>

namespace {

QString q(const rust::String &s) {
    return QString::fromUtf8(s.data(), int(s.size()));
}

rust::String rs(const QString &s) {
    return rust::String(s.toUtf8().toStdString());
}

} // namespace

AiPanel::AiPanel(QWidget *parent) : QWidget(parent) {
    setObjectName(QStringLiteral("aiPanel"));

    auto *col = new QVBoxLayout(this);
    col->setContentsMargins(4, 4, 4, 4);
    col->setSpacing(4);

    auto *row = new QHBoxLayout();
    mode_ = new QComboBox(this);
    mode_->addItems({i18n::tr("命令生成"), i18n::tr("错误解释")});
    input_ = new QLineEdit(this);
    input_->setPlaceholderText(
        i18n::tr("自然语言描述要执行的命令，或粘贴报错文本"));
    ask_ = new QPushButton(i18n::tr("问 AI"), this);
    row->addWidget(mode_);
    row->addWidget(input_, 1);
    row->addWidget(ask_);
    col->addLayout(row);

    candidates_ = new QListWidget(this);
    candidates_->setMaximumHeight(110);
    col->addWidget(candidates_);

    explanation_ = new QPlainTextEdit(this);
    explanation_->setReadOnly(true);
    explanation_->setMaximumHeight(110);
    explanation_->setPlaceholderText(i18n::tr("解释/健康检查结果"));
    col->addWidget(explanation_);

    status_ = new QLabel(i18n::tr("就绪"), this);
    status_->setObjectName(QStringLiteral("mutedHint"));
    col->addWidget(status_);

    QObject::connect(ask_, &QPushButton::clicked, this,
                     [this] { run(mode_->currentIndex() == 1); });
    QObject::connect(input_, &QLineEdit::returnPressed, this,
                     [this] { run(mode_->currentIndex() == 1); });
    // 双击候选 = 发送到终端。
    QObject::connect(candidates_, &QListWidget::itemDoubleClicked, this,
                     [this](QListWidgetItem *item) {
                         if (on_send_command != nullptr) {
                             on_send_command(item->data(Qt::UserRole).toString());
                         }
                     });
}

void AiPanel::run(bool explain) {
    const QString text = input_->text().trimmed();
    if (text.isEmpty()) {
        status_->setText(i18n::tr("先输入内容"));
        return;
    }
    busy_ = true;
    status_->setText(explain ? i18n::tr("解释中…") : i18n::tr("生成中…"));
    if (explain) {
        ai_explain(rs(text));
    } else {
        ai_generate(rs(text));
    }
}

void AiPanel::ask_command(const QString &prompt) {
    mode_->setCurrentIndex(0);
    input_->setText(prompt);
    run(false);
}

void AiPanel::ask_explain(const QString &error_text) {
    mode_->setCurrentIndex(1);
    input_->setText(error_text);
    run(true);
}

void AiPanel::poll() {
    const rust::Vec<AiReplyPod> replies = ai_take_replies();
    for (const AiReplyPod &r : replies) {
        QStringList commands;
        QStringList notes;
        QVector<int> confidences;
        for (size_t i = 0; i < r.commands.size(); ++i) {
            commands << q(r.commands[i]);
            notes << (i < r.notes.size() ? q(r.notes[i]) : QString());
            confidences << (i < r.confidences.size() ? int(r.confidences[i]) : 0);
        }
        render_reply(int(r.kind), r.ok, q(r.explanation), commands, notes,
                     confidences);
        if (on_note != nullptr && !r.ok) {
            on_note(i18n::tr("AI 请求失败（详见面板）"));
        }
    }
    if (!replies.empty()) {
        busy_ = false;
        status_->setText(i18n::tr("就绪"));
    }
}

void AiPanel::render_reply(int kind, bool ok, const QString &explanation,
                           const QStringList &commands, const QStringList &notes,
                           const QVector<int> &confidences) {
    if (kind == 0) {
        candidates_->clear();
        for (int i = 0; i < commands.size(); ++i) {
            auto *item = new QListWidgetItem(
                QStringLiteral("%1  (%2%)\n%3")
                    .arg(commands[i])
                    .arg(confidences.value(i))
                    .arg(notes.value(i)),
                candidates_);
            item->setData(Qt::UserRole, commands[i]);
        }
        if (candidates_->count() == 0) {
            candidates_->addItem(i18n::tr("（无候选）"));
        }
    } else {
        explanation_->setPlainText(explanation);
    }
    status_->setText(ok ? i18n::tr("完成") : i18n::tr("失败（见内容）"));
}

int AiPanel::candidate_count() const { return candidates_->count(); }
QString AiPanel::explanation_text() const { return explanation_->toPlainText(); }
bool AiPanel::busy() const { return busy_; }

// ---- AI 设置对话框 ----

bool run_ai_settings(QWidget *parent) {
    const AiCfgPod cur = ai_config_get();

    QDialog dlg(parent);
    dlg.setWindowTitle(i18n::tr("AI 设置"));
    auto *root = new QVBoxLayout(&dlg);
    auto *form = new QFormLayout;

    auto *provider = new QComboBox(&dlg);
    provider->addItems({QStringLiteral("auto"), QStringLiteral("cloud"),
                        QStringLiteral("ollama")});
    provider->setCurrentText(q(cur.provider).isEmpty()
                                 ? QStringLiteral("auto")
                                 : q(cur.provider));
    form->addRow(QStringLiteral("Provider"), provider);
    auto *base_url = new QLineEdit(q(cur.base_url), &dlg);
    base_url->setPlaceholderText(QStringLiteral("http://localhost:11434"));
    form->addRow(QStringLiteral("Base URL"), base_url);
    auto *model = new QLineEdit(q(cur.model), &dlg);
    form->addRow(i18n::tr("模型"), model);
    auto *privacy = new QCheckBox(i18n::tr("隐私模式（不发送终端内容）"), &dlg);
    privacy->setChecked(cur.privacy_mode);
    form->addRow(QString(), privacy);
    auto *key = new QLineEdit(&dlg);
    key->setEchoMode(QLineEdit::Password);
    key->setPlaceholderText(i18n::tr("API key（只保存在内存，不落盘）"));
    form->addRow(QStringLiteral("API Key"), key);
    root->addLayout(form);

    auto *buttons = new QDialogButtonBox(&dlg);
    buttons->addButton(QDialogButtonBox::Ok);
    buttons->addButton(QDialogButtonBox::Cancel);
    QObject::connect(buttons, &QDialogButtonBox::accepted, &dlg,
                     [&dlg] { dlg.accept(); });
    QObject::connect(buttons, &QDialogButtonBox::rejected, &dlg,
                     [&dlg] { dlg.reject(); });
    root->addWidget(buttons);

    if (dlg.exec() != QDialog::Accepted) {
        return false;
    }

    AiCfgPod cfg;
    cfg.provider = rs(provider->currentText());
    cfg.base_url = rs(base_url->text().trimmed());
    cfg.model = rs(model->text().trimmed());
    cfg.privacy_mode = privacy->isChecked();
    cfg.api_key = rs(key->text()); // 空时不覆盖内存 key
    ai_config_set(cfg);
    return true;
}

// ---- 装配 ----

void wire_ai_panel(MainWindow *win) {
    AiPanel *ai = win->docks() != nullptr ? win->docks()->ai : nullptr;
    if (ai == nullptr) {
        return;
    }
    // AI 候选「发送到终端」：写活动窗格 PTY + 换行（WindTerm 双击候选语义）。
    ai->on_send_command = [win](const QString &command) {
        PtyView *v = win->active_view();
        if (v == nullptr) {
            win->note(i18n::tr("没有活动终端可发送"));
            return;
        }
        const QByteArray bytes = (command + QChar('\n')).toUtf8();
        term_write(v->tab(),
                   rust::Slice<const uint8_t>(
                       reinterpret_cast<const uint8_t *>(bytes.constData()),
                       size_t(bytes.size())));
        win->note(i18n::tr("已发送：%1").arg(command));
    };
    ai->on_note = [win](const QString &msg) { win->note(msg); };
}

AiPanel *ensure_ai_panel(MainWindow *win) {
    ChromeDocks *d = win != nullptr ? win->docks() : nullptr;
    if (d == nullptr || d->bottom == nullptr) return nullptr;
    if (d->ai != nullptr) return d->ai;
    d->ai = new AiPanel(d->bottom);
    d->bottom->setWidget(d->ai);
    wire_ai_panel(win);
    if (d->bottom->isVisible()) d->ai->show();
    return d->ai;
}

// ---- Phase C ⑥ 自检 ----

/// AI 面板自检：面板结构 / 配置往返（key 不回填）/ 异步生成往返（离线走
/// 失败伪候选路径——面板永远有东西渲染）/ busy 清位。
int qt_boot_c6(rust::Str png_out) {
    static int argc = 1;
    static char arg0[] = "native-c6";
    static char *argv[] = {arg0, nullptr};
    QApplication app(argc, argv);
    selftest_ui::SettingsScope settings_scope;
    i18n::install_from_env();

    MainWindow w;
    w.show();

    QTimer poll;
    int waited_ms = 0;
    int verdict = 1;
    bool ok = true;
    bool saw_reply = false;

    QTimer::singleShot(200, [&] {
        const bool lazy_ok = w.docks() != nullptr && w.docks()->ai == nullptr;
        AiPanel *ai = ensure_ai_panel(&w);

        // 1) 面板结构。
        const bool struct_ok = lazy_ok && ai != nullptr &&
                               w.docks()->bottom->widget() == ai;
        std::printf("[cpp] C6-a AI 面板结构: %s\n", struct_ok ? "通过" : "失败");
        ok &= struct_ok;

        // 2) 配置往返：set（含 key）→ get 回读（key 恒空 = 不回填铁律）。
        bool cfg_ok = true;
        {
            AiCfgPod cfg;
            cfg.provider = rust::String("ollama");
            cfg.base_url = rust::String("http://127.0.0.1:1"); // 故意不可达端口
            cfg.model = rust::String("qwen3");
            cfg.privacy_mode = true;
            cfg.api_key = rust::String("in-memory-only");
            ai_config_set(cfg);
            const AiCfgPod back = ai_config_get();
            cfg_ok &= q(back.provider) == QStringLiteral("ollama");
            cfg_ok &= q(back.base_url) == QStringLiteral("http://127.0.0.1:1");
            cfg_ok &= back.privacy_mode;
            cfg_ok &= back.api_key.empty(); // 永不回填
        }
        std::printf("[cpp] C6-b 配置往返(key 不回填): %s\n", cfg_ok ? "通过" : "失败");
        ok &= cfg_ok;

        // 3) 异步生成往返：不可达 provider → 失败伪候选（面板有内容可渲染）。
        if (ai != nullptr) {
            ai->ask_command(QStringLiteral("列出当前目录文件"));
        }
        poll.start(100);
    });

    QObject::connect(&poll, &QTimer::timeout, [&] {
        waited_ms += 100;
        if (w.docks() != nullptr && w.docks()->ai != nullptr) {
            w.docks()->ai->poll();
            if (w.docks()->ai->candidate_count() > 0) {
                saw_reply = true;
            }
        }
        if (saw_reply || waited_ms >= 8000) {
            poll.stop();
            std::printf("[cpp] C6-c 异步生成往返: %s（等 %dms，候选 %d 条）\n",
                        saw_reply ? "通过" : "失败", waited_ms,
                        w.docks() != nullptr && w.docks()->ai != nullptr
                            ? w.docks()->ai->candidate_count()
                            : -1);
            ok &= saw_reply;

            // 4) busy 清位。
            const bool idle_ok = w.docks() != nullptr && w.docks()->ai != nullptr
                                 && !w.docks()->ai->busy();
            std::printf("[cpp] C6-d busy 清位: %s\n", idle_ok ? "通过" : "失败");
            ok &= idle_ok;

            const std::string png_path(png_out.data(), png_out.size());
            if (!png_path.empty()) {
                w.docks()->bottom->show();
                const QPixmap shot = w.grab();
                const bool saved = shot.save(QString::fromStdString(png_path));
                std::printf("[cpp] 离屏渲染落盘 %s: %s (%dx%d)\n", png_path.c_str(),
                            saved ? "成功" : "失败", shot.width(), shot.height());
                ok = ok && saved;
            }
            verdict = ok ? 0 : 1;
            std::printf("[cpp] Phase C ⑥ 自检: %s\n", verdict == 0 ? "通过" : "失败");
            app.exit(verdict);
        }
    });

    return app.exec();
}
