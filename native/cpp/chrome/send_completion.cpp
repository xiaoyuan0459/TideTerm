#include "cpp/chrome/send_completion.h"
#include "cpp/chrome/completion_labels.h"
#include "cpp/chrome/send_programs.h"
#include "cpp/main_window.h"
#include <QSignalBlocker>
#include <QDynamicPropertyChangeEvent>
#include "cpp/chrome/send_shell_report.h"
#include "cpp/bridge_shim.h"
#include "cpp/chrome/icon_cache.h"
#include "cpp/i18n.h"
#include <QApplication>
#include <QComboBox>
#include <QGuiApplication>
#include <QHBoxLayout>
#include <QInputMethodEvent>
#include <QKeyEvent>
#include <QLineEdit>
#include <QListWidget>
#include <QPointer>
#include <QScreen>
#include <QTimer>
#include <QToolButton>
#include <algorithm>

namespace {
QString q(const rust::String &s) { return QString::fromUtf8(s.data(), int(s.size())); }
QString shell_kind(uint8_t kind) {
    switch (kind) {
    case 1: return i18n::tr("别名");
    case 2: return i18n::tr("函数");
    case 3: return i18n::tr("内建命令");
    case 4: return i18n::tr("关键字");
    case 5: return i18n::tr("程序");
    case 6: return i18n::tr("外部脚本");
    case 7: return QStringLiteral("Cmdlet");
    case 8: return QStringLiteral("Filter");
    case 9: return QStringLiteral("Configuration");
    default: return i18n::tr("未找到");
    }
}
QString parameter_hint(const QString &kind) {
    if (kind == QLatin1String("container")) return i18n::tr("容器名");
    if (kind == QLatin1String("service")) return i18n::tr("服务名");
    if (kind == QLatin1String("count")) return i18n::tr("数量");
    if (kind == QLatin1String("namespace")) return i18n::tr("命名空间");
    if (kind == QLatin1String("format")) return i18n::tr("输出格式");
    if (kind == QLatin1String("resource")) return i18n::tr("资源类型");
    if (kind == QLatin1String("revision")) return i18n::tr("提交或分支");
    return kind;
}
QString environment_message(const EnvironmentBatch &result) {
    switch (result.state) {
    case 1: return i18n::tr("正在查找会话建议…");
    case 2:
        if (result.truncated) return i18n::tr("仅显示部分会话建议，请缩小查询或目录范围。");
        if (result.skipped) return i18n::tr("已省略需要引用或无法直接插入的名称。");
        return result.items.empty() ? i18n::tr("未找到匹配的会话建议。") : QString();
    case 3: return i18n::tr("此会话暂不支持路径建议；不会改用本机目录。");
    case 4: return i18n::tr("等待会话报告工作目录。");
    case 5: return i18n::tr("仅支持简单草稿末尾的显式路径；引用参数请先选择对应 Shell 语法。");
    case 6: return i18n::tr("无法读取路径，请检查目录和访问权限后重试。");
    case 7: return i18n::tr("路径扫描超时，请重试。");
    case 8: return i18n::tr("路径建议暂时繁忙，请关闭其他建议后重试。");
    case 9: return i18n::tr("路径建议来源已改变，请重新打开建议。");
    case 10: return i18n::tr("远端 SFTP 路径服务不可用，请检查服务器后重试；不会改用本机目录。");
    case 11: return i18n::tr("此 SSH 会话已禁用 SFTP，请先在会话设置中启用。");
    case 12: return i18n::tr("请先配置当前会话的程序目录。");
    case 13: return i18n::tr("程序目录需为该会话的绝对路径，最多 8 个、合计 16 KiB；不支持 UNC、设备路径或控制字符。");
    case 14: return i18n::tr("程序目录属于另一会话或连接版本，请重新配置。");
    default: return {};
    }
}
class Suggestions final : public QObject {
public:
    Suggestions(QWidget *owner, QHBoxLayout *row, QLineEdit *input,
                std::function<uint32_t()> source, std::function<bool()> enabled)
        : QObject(owner), input_(input), source_(std::move(source)), enabled_(std::move(enabled)) {
        popup_ = new QListWidget(owner);
        popup_->setObjectName(QStringLiteral("sendSuggestions"));
        popup_->setWindowFlags(Qt::ToolTip | Qt::WindowDoesNotAcceptFocus);
        popup_->setFocusPolicy(Qt::NoFocus);
        popup_->setUniformItemSizes(true);
        popup_->setTextElideMode(Qt::ElideRight);
        popup_->setHorizontalScrollBarPolicy(Qt::ScrollBarAlwaysOff);
        popup_->setAccessibleName(i18n::tr("命令建议（仅填入草稿）"));
        popup_->hide();
        input_->installEventFilter(this);
        owner->installEventFilter(this);
        connect(input_, &QLineEdit::textEdited, this, [this] {
            if (input_->text().isEmpty()) close(); else show();
        });
        connect(input_, &QLineEdit::textChanged, this, [this] { if (input_->text() != query_) close(); });
        connect(popup_, &QListWidget::itemClicked, this, [this](QListWidgetItem *) { accept(); });
        timer_ = new QTimer(this);
        timer_->setInterval(200);
        connect(timer_, &QTimer::timeout, this, [this] {
            const bool same_draft = enabled_() && input_->isVisible() && input_->hasFocus()
                && input_->text() == query_ && source_() == batch_.context.source;
            if (!same_draft) { close(); return; }
            // A late terminal event can change the completion context after this
            // request started. Keep the visible draft and query again rather than
            // silently retiring the list the user is still editing.
            if (!completion_valid(batch_.context)) { show(); return; }
            if (!batch_.shell.token.empty() && !completion_shell_valid(batch_.context, batch_.shell.token)) { close(); return; }
            if (environment_.state == 1) {
                auto next = environment_poll(environment_owner_);
                if (next.state != 1) {
                    environment_ = std::move(next);
                    const auto text = query_.toUtf8();
                    batch_ = completion_environment_query(source_(), rust::Str(text.constData(), size_t(text.size())), environment_owner_);
                    render();
                }
            }
        });
        auto *browse = new QToolButton(owner);
        browse->setObjectName(QStringLiteral("sendBrowseCommands"));
        chrome::set_button_icon(browse, QStringLiteral("down"), 12);
        browse->setToolTip(i18n::tr("历史与用户命令（Ctrl+Space）"));
        browse->setAccessibleName(browse->toolTip());
        connect(browse, &QToolButton::clicked, this, [this] { input_->setFocus(); show(); });
        row->addWidget(browse);
        auto *manage = new QToolButton(owner);
        manage->setObjectName(QStringLiteral("sendManageCommands"));
        chrome::set_button_icon(manage, QStringLiteral("settings"), 12);
        manage->setToolTip(i18n::tr("管理用户命令…"));
        manage->setAccessibleName(manage->toolTip());
        connect(manage, &QToolButton::clicked, this, [this, owner] {
            close(); chrome::edit_user_commands(owner); chrome::refresh_send_quickbar(owner); input_->setFocus();
        });
        row->addWidget(manage);
        chrome::install_shell_report(owner, row, source_, [this] { close(); }, enabled_, input_);
        environment_button_ = new QToolButton(owner);
        environment_button_->setObjectName(QStringLiteral("sendEnvironmentPaths"));
        environment_button_->setText(i18n::tr("路径建议"));
        environment_button_->setCheckable(true);
        environment_button_->setToolTip(i18n::tr("可选会话路径建议：本地目录或当前 SSH 的 SFTP，只填草稿。本地会话进入嵌套远端 Shell 后请关闭。"));
        environment_button_->setAccessibleName(environment_button_->toolTip());
        connect(environment_button_, &QToolButton::clicked, this, [this] {
            close();
            if (!environment_button_->isChecked()) { environment_close(environment_owner_); environment_owner_ = 0; }
            publish_model(); input_->setFocus(); show();
        });
        row->addWidget(environment_button_);
        syntax_ = new QComboBox(owner);
        syntax_->setObjectName(QStringLiteral("sendPathSyntax"));
        syntax_->addItems({i18n::tr("保守路径"), QStringLiteral("POSIX"), QStringLiteral("PowerShell")});
        syntax_->setToolTip(i18n::tr("按目标 Shell 选择参数路径语法，当前会话的发送栏与终端建议共用。不支持 cmd/batch、嵌套解释器或命令位置；PowerShell 通配符名称仍省略。"));
        syntax_->setAccessibleName(i18n::tr("路径引用语法"));
        syntax_->setEnabled(false);
        connect(environment_button_, &QToolButton::toggled, syntax_, &QWidget::setEnabled);
        connect(syntax_, &QComboBox::currentIndexChanged, this, [this] {
            close(); publish_model(); input_->setFocus(); show();
        });
        row->addWidget(syntax_);
        programs_ = new chrome::ProgramSettings(owner, row, source_, [this] { close(); }, [this] {
            close(); syntax_->setEnabled(environment_button_->isChecked() || programs_->enabled());
            publish_model();
            input_->setFocus(); QTimer::singleShot(0, this, [this] { show(); });
        }, [](uint8_t state) { EnvironmentBatch result; result.state = state; return environment_message(result); });
        connect(environment_button_, &QToolButton::toggled, this, [this] {
            syntax_->setEnabled(environment_button_->isChecked() || programs_->enabled());
        });
    }
    ~Suggestions() override { environment_close(environment_owner_); }
protected:
    bool eventFilter(QObject *watched, QEvent *event) override {
        if (watched == parent()) {
            if (event->type() == QEvent::DynamicPropertyChange) {
                const auto name = static_cast<QDynamicPropertyChangeEvent *>(event)->propertyName();
                if (name == "completionSource" || name == "completionSettingsRevision") sync_model();
            } else if (event->type() == QEvent::Show) sync_model();
            else if (event->type() == QEvent::Hide) close();
            return false;
        }
        if (event->type() == QEvent::InputMethod) {
            composing_ = !static_cast<QInputMethodEvent *>(event)->preeditString().isEmpty();
            if (composing_) close();
        } else if (event->type() == QEvent::FocusIn) {
            // Modal configuration can restore window focus after its refresh callback.
            // Resume the same visible draft when focus actually returns to the editor.
            if (!input_->text().isEmpty()) QTimer::singleShot(0, this, [this] { show(); });
        } else if (event->type() == QEvent::FocusOut || event->type() == QEvent::Hide) close();
        else if (event->type() == QEvent::KeyPress && !composing_) {
            const auto *key = static_cast<QKeyEvent *>(event);
            // Each send/accept requires a fresh press. This also protects unchanged drafts
            // and returning from modal editors; intentional repeats use the sender count.
            const bool accepting = key->key() == Qt::Key_Return || key->key() == Qt::Key_Enter || key->key() == Qt::Key_Tab;
            if (key->isAutoRepeat() && accepting) return true;
            if (key->key() == Qt::Key_Space && key->modifiers() == Qt::ControlModifier) {
                show(); return true;
            }
            if (!popup_->isVisible()) return false;
            if (key->key() == Qt::Key_Escape) { close(); return true; }
            if (key->key() == Qt::Key_Return || key->key() == Qt::Key_Enter || key->key() == Qt::Key_Tab) {
                accept(); return true;
            }
            if (key->key() == Qt::Key_Down || key->key() == Qt::Key_Up) {
                const int step = key->key() == Qt::Key_Down ? 1 : -1;
                if (!batch_.items.empty()) popup_->setCurrentRow(std::clamp(popup_->currentRow() + step, 0, int(batch_.items.size()) - 1));
                return true;
            }
            if (key->key() == Qt::Key_Backtab) { close(); return false; }
        }
        return false;
    }
private:
    void sync_model() {
        const auto source = source_(); const auto revision = completion_sources_revision(source);
        if (source == shared_source_ && revision == shared_revision_) return;
        close();
        auto settings = completion_sources_get(source);
        const QSignalBlocker paths(environment_button_), syntax(syntax_);
        environment_button_->setChecked(settings.paths); syntax_->setCurrentIndex(settings.dialect);
        programs_->sync(std::move(settings.programs), settings.programs_enabled);
        syntax_->setEnabled(settings.paths || settings.programs_enabled);
        shared_source_ = source; shared_revision_ = revision;
    }
    void publish_model() {
        const auto context = completion_context(source_());
        const auto error = completion_sources_set(context, environment_button_->isChecked(), uint8_t(syntax_->currentIndex()), programs_->enabled(), programs_->settings());
        if (error) {
            EnvironmentBatch result; result.state = error;
            if (auto *window = dynamic_cast<MainWindow *>(input_->window())) window->note(environment_message(result));
            shared_source_ = UINT32_MAX; sync_model();
        } else shared_revision_ = completion_sources_revision(source_());
    }
    bool valid() const {
        return enabled_() && input_->isVisible() && input_->hasFocus()
            && input_->text() == query_ && source_() == batch_.context.source && completion_valid(batch_.context);
    }
    void close() { popup_->hide(); timer_->stop(); environment_cancel(environment_owner_); }
    void show() {
        close();
        sync_model();
        if (composing_ || !enabled_() || !input_->hasFocus() || !input_->isVisible()) return;
        const QByteArray text = input_->text().toUtf8();
        query_ = input_->text();
        batch_ = completion_query(source_(), rust::Str(text.constData(), size_t(text.size())));
        environment_ = EnvironmentBatch{};
        if (environment_button_->isChecked() || programs_->enabled()) {
            if (!environment_owner_) environment_owner_ = environment_create();
            const rust::Str query(text.constData(), size_t(text.size()));
            if (programs_->enabled()) environment_ = programs_->request(environment_owner_, batch_.context, query, uint8_t(syntax_->currentIndex()));
            if (environment_.state == 0 && environment_button_->isChecked())
                environment_ = environment_request(environment_owner_, batch_.context, query, uint8_t(syntax_->currentIndex()));
            if (environment_.state == 2) batch_ = completion_environment_query(source_(), rust::Str(text.constData(), size_t(text.size())), environment_owner_);
        }
        render();
    }
    void render() {
        const auto *selected = popup_->currentItem();
        const QString selected_command = selected ? selected->data(Qt::UserRole).toString() : QString();
        const int selected_kind = selected ? selected->data(Qt::UserRole + 1).toInt() : -1;
        const uint32_t selected_user = selected ? selected->data(Qt::UserRole + 2).toUInt() : 0;
        popup_->clear();
        int selected_row = -1;
        for (const auto &entry : batch_.items) {
            const QString origin = entry.kind == 0 ? i18n::tr("本终端历史")
                : entry.kind == 2 ? i18n::tr("参数规则") : entry.kind == 3 ? i18n::tr("本地路径")
                : entry.kind == 4 ? i18n::tr("SSH 路径")
                : entry.kind == 5 ? i18n::tr("本地程序目录") : entry.kind == 6 ? i18n::tr("SSH 程序目录")
                : entry.kind == 7 ? i18n::tr("Shell 解析（显式查询）")
                : entry.kind == 8 ? i18n::tr("Git 分支（显式快照）") : entry.kind == 9 ? i18n::tr("Git 标签（显式快照）")
                : entry.kind == 10 ? i18n::tr("Docker 容器（显式快照）")
                : entry.kind == 11 ? i18n::tr("kubectl 资源（显式快照）")
                : entry.kind == 12 ? i18n::tr("Shell 名字目录（显式采集）")
                : entry.kind == 13 ? i18n::tr("命令词库（参考）")
                : entry.is_template ? i18n::tr("用户模板") : i18n::tr("用户命令");
            const QString label = entry.kind == 12 ? shell_kind(uint8_t(q(entry.detail).toUInt())) : entry.kind == 7 ? shell_kind(batch_.shell.kind)
                : entry.kind == 2 ? parameter_hint(q(entry.label)) : q(entry.label);
            auto *item = new QListWidgetItem(origin + QStringLiteral(" · ")
                + (label.isEmpty() ? QString() : label + QStringLiteral(" — ")) + q(entry.command), popup_);
            item->setData(Qt::UserRole, q(entry.command)); item->setData(Qt::UserRole + 1, entry.kind);
            item->setData(Qt::UserRole + 2, entry.user_id);
            if (q(entry.command) == selected_command && int(entry.kind) == selected_kind && entry.user_id == selected_user) selected_row = popup_->count() - 1;
            item->setToolTip(q(entry.command) + QStringLiteral("\n") + i18n::tr(q(entry.description))
                + QStringLiteral("\n") + q(entry.detail) + QStringLiteral("\n")
                + (entry.kind == 2 ? i18n::tr("规则不代表目标程序已安装或版本匹配；动态参数需自行填写。") : i18n::tr("仅填入草稿，发送需再次确认。")));
            if (entry.kind == 3) item->setToolTip(i18n::tr("本地目录快照，路径可能已变化；只填草稿，不保证目标命令可执行。")
                + QStringLiteral("\n") + q(batch_.context.cwd) + QStringLiteral("\n") + q(entry.command));
            if (entry.kind == 4) item->setToolTip(i18n::tr("当前 SSH 的 SFTP 目录快照，路径可能已变化；只填草稿，不保证目标命令可执行。")
                + QStringLiteral("\n") + q(batch_.context.cwd) + QStringLiteral("\n") + q(entry.command));
            if (entry.kind == 5 || entry.kind == 6) item->setToolTip(i18n::tr("指定程序目录元数据快照；不代表 Shell 的 PATH、别名、实际可执行性或版本。只填完整路径草稿。")
                + QStringLiteral("\n") + q(entry.detail) + QStringLiteral("\n") + q(entry.command));
            if (entry.kind == 7) item->setToolTip(i18n::tr("仅复用最近一次显式查询，60 秒内有效；不自动查询或保证可执行性。选择消费回执，只填名字草稿。")
                + QStringLiteral("\n") + (batch_.shell.platform == 1 ? QStringLiteral("Bash") : QStringLiteral("PowerShell"))
                + QStringLiteral(" · ") + shell_kind(batch_.shell.kind) + QStringLiteral("\n") + q(batch_.shell.location)
                + QStringLiteral("\n") + i18n::tr("会话 %1，连接 %2，目录版本 %3，已过 %4 秒")
                    .arg(batch_.context.source).arg(batch_.context.generation).arg(batch_.context.directory_revision).arg(batch_.shell.age_seconds)
                + QStringLiteral("\n") + q(batch_.context.cwd));
            if (entry.kind == 8 || entry.kind == 9) item->setToolTip(i18n::tr("仅使用最近一次显式 Git 快照，60 秒内有效；引用可能已变化，不自动刷新。选择只填草稿。")
                + QStringLiteral("\n") + q(entry.detail) + QStringLiteral("\n") + q(entry.reference)
                + QStringLiteral("\n") + i18n::tr("会话 %1，连接 %2，目录版本 %3，已过 %4 秒")
                    .arg(batch_.context.source).arg(batch_.context.generation).arg(batch_.context.directory_revision).arg(batch_.shell.age_seconds));
            if (entry.kind == 10) item->setToolTip(i18n::tr("Docker 短时快照；只填带明确 context 和完整 ID 的草稿，不自动刷新或读取容器内容。")
                + QStringLiteral("\ncontext: ") + q(entry.detail) + QStringLiteral("\n") + q(entry.reference)
                + QStringLiteral("\n") + i18n::tr("会话 %1，连接 %2，目录版本 %3，已过 %4 秒")
                    .arg(batch_.context.source).arg(batch_.context.generation).arg(batch_.context.directory_revision).arg(batch_.shell.age_seconds));
            if (entry.kind == 11) {
                const QString tip = i18n::tr("资源快照只填草稿；名称可被重建，发送时不保证仍为此 UID。") + QLatin1Char('\n') + q(entry.detail) + QLatin1Char('\n') + q(entry.reference);
                item->setToolTip(QStringLiteral("<qt>") + tip.toHtmlEscaped().replace(QLatin1Char('\n'), QStringLiteral("<br>")) + QStringLiteral("</qt>"));
            }
            if (entry.kind == 13) {
                const auto context=i18n::tr("当前环境：%1 / %2；来源：%3").arg(chrome::completion_shell_label(batch_.context.shell_kind),chrome::completion_system_label(batch_.context.system),origin);
                const auto tip=q(entry.command)+QLatin1Char('\n')+context+QLatin1Char('\n')+i18n::tr(q(entry.description))+QLatin1Char('\n')+q(entry.detail)+QLatin1Char('\n')+i18n::tr("命令参考不代表当前已安装；仅填入草稿。");
                item->setToolTip(QStringLiteral("<qt>")+tip.toHtmlEscaped().replace(QLatin1Char('\n'),QStringLiteral("<br>"))+QStringLiteral("</qt>"));
            }
        }
        if (batch_.shell.git && (batch_.shell.truncated || batch_.items.empty())) {
            auto *status = new QListWidgetItem(batch_.shell.truncated ? i18n::tr("仅显示部分 Git 引用，请缩小仓库范围后重新采集。") : i18n::tr("Git 快照中没有匹配的引用。"), popup_); status->setFlags(Qt::NoItemFlags);
        }
        if (!batch_.shell.docker_context.empty() && (batch_.shell.truncated || batch_.items.empty())) {
            auto *status = new QListWidgetItem(batch_.shell.truncated ? i18n::tr("仅显示部分近期 Docker 容器，请重新采集或缩小候选前缀。") : i18n::tr("Docker 快照中没有匹配的容器。"), popup_); status->setFlags(Qt::NoItemFlags);
        }
        if (!batch_.shell.kube.config.empty() && (batch_.shell.truncated || batch_.items.empty())) {
            auto *status = new QListWidgetItem(batch_.shell.truncated ? i18n::tr("仅保留部分资源；不代表完整 namespace 列表。") : i18n::tr("资源快照没有匹配项。"), popup_); status->setFlags(Qt::NoItemFlags);
        }
        if (batch_.shell.name_catalog && (batch_.shell.truncated || batch_.items.empty())) {
            auto *status = new QListWidgetItem(batch_.shell.truncated ? i18n::tr("仅保留部分适用名字；可填写更具体的采集前缀后刷新。") : i18n::tr("名字目录没有匹配项，可从 Shell 环境重新采集。"), popup_); status->setFlags(Qt::NoItemFlags);
        }
        if (batch_.shell.state == 6 || batch_.shell.state == 7) {
            auto *status = new QListWidgetItem(i18n::tr("来源快照已失效，请从 Shell 环境重新采集。"), popup_); status->setFlags(Qt::NoItemFlags);
        }
        if (batch_.shell.state == 3 && !batch_.shell.name_catalog && !batch_.shell.git && batch_.shell.docker_context.empty() && batch_.shell.kube.config.empty() && !batch_.shell.can_fill) {
            const QString message = i18n::tr("Shell 显式查询：%1 · %2（不可填入）").arg(q(batch_.shell.query), shell_kind(batch_.shell.kind));
            auto *status = new QListWidgetItem(message, popup_); status->setFlags(Qt::NoItemFlags); status->setToolTip(message);
        }
        if (batch_.rule_error) { auto *error = new QListWidgetItem(i18n::tr("离线规则不可用"), popup_); error->setFlags(Qt::NoItemFlags); }
        const auto message = environment_message(environment_);
        if (!message.isEmpty()) { auto *status = new QListWidgetItem(message, popup_); status->setFlags(Qt::NoItemFlags); status->setToolTip(message); }
        if (!popup_->count() || !valid()) return;
        popup_->setCurrentRow(batch_.items.empty() ? -1 : std::max(0, selected_row));
        const int height = std::min(240, popup_->sizeHintForRow(0) * popup_->count() + 6);
        const QRect screen = input_->screen()->availableGeometry();
        const int width = std::min(std::max(input_->width(), 320), screen.width());
        const QPoint below = input_->mapToGlobal(QPoint(0, input_->height()));
        const int y = below.y() + height <= screen.bottom() ? below.y()
            : std::max(screen.top(), input_->mapToGlobal(QPoint(0, 0)).y() - height);
        popup_->setGeometry(std::clamp(below.x(), screen.left(), screen.right() - width + 1), y, width, height);
        popup_->show(); timer_->start();
    }
    void accept() {
        const int at = popup_->currentRow();
        if (!valid() || at < 0 || size_t(at) >= batch_.items.size()) { close(); return; }
        const auto &entry = batch_.items[size_t(at)];
        const auto query = query_.toUtf8();
        if (entry.kind >= 3 && entry.kind <= 6 && !environment_valid(environment_owner_, environment_.request, batch_.context,
                rust::Str(query.constData(), size_t(query.size())))) { close(); return; }
        const QString text = entry.kind == 12 ? q(completion_name_draft(batch_.context, batch_.shell.token, entry.reference))
            : entry.kind == 11 ? q(completion_kube_draft(batch_.context, batch_.shell.token,
                rust::Str(query.constData(), size_t(query.size())), entry.reference))
            : entry.kind == 10 ? q(completion_docker_draft(batch_.context, batch_.shell.token,
                rust::Str(query.constData(), size_t(query.size())), entry.reference))
            : (entry.kind == 8 || entry.kind == 9) ? q(completion_git_draft(batch_.context, batch_.shell.token,
                rust::Str(query.constData(), size_t(query.size())), entry.reference))
            : entry.kind == 7 ? q(completion_shell_draft(batch_.context, batch_.shell.token)) : q(entry.command);
        if (text.isEmpty()) { close(); return; }
        const auto context = batch_.context; const auto id = entry.user_id; const bool is_template = entry.is_template;
        close();
        if (is_template) {
            const QPointer<QObject> guard(this); const QPointer<QLineEdit> input(input_);
            const auto enabled = enabled_; const auto source = source_;
            chrome::fill_user_command(input_->window(), input_, context, id, [guard, input, enabled, source, &context] {
                return guard && input && enabled() && input->isVisible() && source() == context.source;
            });
        } else { input_->setText(text); input_->setCursorPosition(text.size()); }
    }
    QLineEdit *input_;
    uint32_t shared_source_ = UINT32_MAX;
    uint64_t shared_revision_ = 0;
    QListWidget *popup_;
    QTimer *timer_;
    QToolButton *environment_button_;
    QComboBox *syntax_;
    chrome::ProgramSettings *programs_;
    uint64_t environment_owner_ = 0;
    EnvironmentBatch environment_;
    std::function<uint32_t()> source_;
    std::function<bool()> enabled_;
    CompletionBatch batch_;
    bool composing_ = false;
    QString query_;
};
}
namespace chrome {
void install_send_completion(QWidget *owner, QHBoxLayout *row, QLineEdit *input,
                             std::function<uint32_t()> source, std::function<bool()> enabled) {
    new Suggestions(owner, row, input, std::move(source), std::move(enabled));
}
}
