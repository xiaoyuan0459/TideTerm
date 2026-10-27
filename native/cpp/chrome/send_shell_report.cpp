#include "cpp/chrome/send_shell_report.h"
#include "cpp/chrome/completion_labels.h"
#include "cpp/bridge_shim.h"
#include "cpp/i18n.h"
#include <QCheckBox>
#include <QComboBox>
#include <QDialog>
#include <QDialogButtonBox>
#include <QFontDatabase>
#include <QHBoxLayout>
#include <QLabel>
#include <QLineEdit>
#include <QPlainTextEdit>
#include <QPointer>
#include <QPushButton>
#include <QTimer>
#include <QToolButton>
#include <QVBoxLayout>
#include <QWindow>

namespace {
QString q(const rust::String &s) { return QString::fromUtf8(s.data(), int(s.size())); }
QString state_text(const ShellReport &r) {
    switch (r.state) {
    case 0: return i18n::tr("尚未采集 Shell 环境。");
    case 1: return i18n::tr("命令已预览；确认当前终端为空提示符后单独发送。");
    case 2: return i18n::tr("正在等待此终端的环境报告（15 秒）…");
    case 3: if (r.name_catalog) return i18n::tr("已采集 %1 个命令名字，%2 秒前；60 秒有效。关闭后输入名字片段筛选，接受只填草稿。").arg(r.names.size()).arg(r.age_seconds);
        if (!r.kube.config.empty()) return i18n::tr("资源快照：%1 秒前；60 秒有效。关闭后输入 kubectl get / describe、所选资源类型和名字前缀填稿。").arg(r.age_seconds);
        if (!r.docker_context.empty()) return i18n::tr("Docker 快照：%1 秒前；60 秒有效。关闭后用 docker inspect / docker logs 加空格填稿。").arg(r.age_seconds);
        if (r.git) return i18n::tr("Git 引用快照：%1 秒前采集，60 秒后失效。关闭后输入 git switch 或 git show 并留一个空格；仅填草稿。").arg(r.age_seconds);
        return (r.query.empty() ? i18n::tr("当前终端通道的 Shell 自报 PATH；%1 秒前采集，60 秒后失效。")
        : i18n::tr("当前 Shell 的已加载命令与 PATH 解析自报；%1 秒前采集，60 秒后失效。")).arg(r.age_seconds);
    case 4: return i18n::tr("等待报告超时；检查 Shell 语法后重新预览。");
    case 5: return i18n::tr("请明确选择 Bash 或 PowerShell。");
    case 7: return i18n::tr("环境快照已过期，请重新采集。");
    case 8: return i18n::tr("报告命令未能进入发送队列，请重试。");
    case 9: return i18n::tr("命令名须以 ASCII 字母或下划线开头，其余只含字母、数字、点、下划线或连字符，最多 128 字节。");
    case 10: return i18n::tr("Git 不可用、当前目录不是仓库、读取失败或超过结果预算；检查终端后重新预览。");
    case 11: return i18n::tr("Docker 不可用、上下文或认证失败、读取失败或结果超限；检查终端后重新预览。");
    case 12: return i18n::tr("请填写明确的 Docker context：1–64 个 ASCII 字符，字母或数字开头，其余可含点、下划线和连字符。");
    case 13: return i18n::tr("请填写单个绝对 kubeconfig 路径、受限 context 和 DNS namespace；不接受控制字符、双引号或智能引号。");
    case 14: return i18n::tr("kubectl、配置、认证或 API 查询失败，或结果超限；请检查终端后重新预览。");
    case 15: return i18n::tr("发送前须单独确认 kubeconfig 的凭据插件与缓存副作用。");
    case 16: return i18n::tr("命令名字采集失败；请检查所选 Shell 和终端输出后重新预览。");
    case 17: return i18n::tr("采集前缀最多 64 个 ASCII 字母、数字、点、下划线或连字符，可留空。");
    default: return i18n::tr("来源已关闭、改变或不可输入，请重新打开。");
    }
}
QString category(uint8_t kind) {
    switch (kind) {
    case 0: return i18n::tr("未找到（当前已加载作用域）");
    case 1: return i18n::tr("别名"); case 2: return i18n::tr("函数");
    case 3: return i18n::tr("Shell 内建"); case 4: return i18n::tr("关键字（仅显示）");
    case 5: return i18n::tr("应用程序"); case 6: return i18n::tr("外部脚本");
    case 7: return QStringLiteral("Cmdlet"); case 8: return i18n::tr("筛选函数");
    default: return i18n::tr("其他类别（仅显示）");
    }
}
void show_report(QWidget *owner, std::function<uint32_t()> source, std::function<bool()> enabled, QLineEdit *input) {
    const QPointer<QWidget> alive(owner); const QPointer<QLineEdit> draft(input);
    const QString original = input->text();
    const auto tab = source(); const auto generation = completion_context(tab).generation;
    QDialog dialog; dialog.setWindowModality(Qt::ApplicationModal); dialog.winId();
    if (owner->window()->windowHandle()) dialog.windowHandle()->setTransientParent(owner->window()->windowHandle());
    QObject::connect(owner, &QObject::destroyed, &dialog, &QDialog::reject);
    QObject::connect(input, &QObject::destroyed, &dialog, &QDialog::reject);
    dialog.setObjectName(QStringLiteral("shellReportDialog"));
    dialog.setWindowTitle(i18n::tr("交互 Shell 环境")); dialog.resize(740, 580);
    auto *layout = new QVBoxLayout(&dialog);
    auto *hint = new QLabel(i18n::tr("仅在所选 Shell 的空提示符预览并显式发送固定查询，不要向 TUI、密码或已有输入发送。命令查询只看当前已加载命令与 PATH，不自动导入模块或运行查询对象，不认证嵌套主机，也不保证实际可执行性或版本。"), &dialog);
    hint->setWordWrap(true); layout->addWidget(hint);
    auto *mode_row = new QHBoxLayout;
    auto *mode = new QComboBox(&dialog); mode->setObjectName(QStringLiteral("shellReportMode"));
    mode->addItems({i18n::tr("PATH 快照"), i18n::tr("精确命令名"), i18n::tr("Git 分支与标签"), i18n::tr("Docker 容器"), i18n::tr("kubectl 资源"), i18n::tr("命令名字目录")});
    auto *names_hint = new QLabel(i18n::tr("显式采集当前 Shell 的已加载名字和可发现程序，最多 256 项/6144 字节名字文本。前缀可缩小采集范围；不自动导入模块或执行候选。不适用或超限的名字会省略，输入筛选不会重新查询。"), &dialog);
    names_hint->setWordWrap(true); layout->addWidget(names_hint); names_hint->hide();
    auto *git_hint = new QLabel(i18n::tr("Git 查询会在此终端执行 PATH 中的 Git 应用程序，读取当前 Git 环境选择的仓库与配置。最多 64 个引用、4096 字节；可能只返回部分。取消或 15 秒超时仅停止接收，不中断 Shell 或 Git；不会自动刷新或改用本机仓库。"), &dialog);
    git_hint->setWordWrap(true); layout->addWidget(git_hint); git_hint->hide();
    auto *docker_hint = new QLabel(i18n::tr("仅在空提示符确认发送。会运行此终端的 Docker，通过所填 context 连接守护进程，可能读取 TLS 凭据或启动 SSH 认证。最多 32 个近期容器、6144 字节；只填草稿。取消或超时不停止子进程；上下文配置改变需重新采集。"), &dialog);
    docker_hint->setWordWrap(true); layout->addWidget(docker_hint); docker_hint->hide();
    auto *query = new QLineEdit(&dialog); query->setObjectName(QStringLiteral("shellLookupName")); query->setMaxLength(128);
    query->setPlaceholderText(i18n::tr("命令名，不含路径或参数；编辑后点击重新预览")); query->setEnabled(false);
    query->setAccessibleName(i18n::tr("精确查询的命令名"));
    auto *context_label = new QLabel(QStringLiteral("context"), &dialog); context_label->setBuddy(query);
    mode_row->addWidget(mode); mode_row->addWidget(context_label); mode_row->addWidget(query, 1); layout->addLayout(mode_row);
    auto *config = new QLineEdit(&dialog); config->setObjectName(QStringLiteral("kubeConfigPath")); config->setMaxLength(512);
    config->setPlaceholderText(i18n::tr("单个绝对 kubeconfig 路径；Windows 原生 CLI 用 C:/…")); config->setAccessibleName(config->placeholderText());
    auto *config_label = new QLabel(QStringLiteral("kubeconfig"), &dialog); config_label->setBuddy(config);
    auto *config_row = new QHBoxLayout; config_row->addWidget(config_label); config_row->addWidget(config, 1); layout->addLayout(config_row);
    auto *ns = new QLineEdit(&dialog); ns->setObjectName(QStringLiteral("kubeNamespace")); ns->setMaxLength(63);
    ns->setPlaceholderText(i18n::tr("明确的 namespace，不猜默认值")); ns->setAccessibleName(ns->placeholderText());
    auto *ns_label = new QLabel(QStringLiteral("namespace"), &dialog); ns_label->setBuddy(ns);
    auto *ns_row = new QHBoxLayout; ns_row->addWidget(ns_label); ns_row->addWidget(ns, 1); layout->addLayout(ns_row);
    auto *resource = new QComboBox(&dialog); resource->setObjectName(QStringLiteral("kubeResource"));
    resource->addItems({QStringLiteral("Pod"), QStringLiteral("Service"), QStringLiteral("Deployment")});
    resource->setAccessibleName(i18n::tr("采集的 kubectl 资源类型")); layout->addWidget(resource);
    auto *kube_hint = new QLabel(i18n::tr("只列指定 namespace 和类型的资源元数据。查询可启动 kubeconfig 凭据插件、访问身份服务并写缓存；须信任配置。CLI 可能载入完整列表，取消不杀进程。PowerShell 需要 7.2+。"), &dialog);
    kube_hint->setWordWrap(true); layout->addWidget(kube_hint);
    auto *syntax = new QComboBox(&dialog); syntax->setObjectName(QStringLiteral("shellReportSyntax"));
    syntax->addItems({i18n::tr("选择 Shell…"), QStringLiteral("Bash"), QStringLiteral("PowerShell")});
    syntax->setAccessibleName(i18n::tr("环境报告语法")); layout->addWidget(syntax);
    auto *preview = new QPlainTextEdit(&dialog); preview->setReadOnly(true);
    preview->setObjectName(QStringLiteral("shellReportPreview")); preview->setAccessibleName(i18n::tr("固定报告命令预览"));
    preview->setFont(QFontDatabase::systemFont(QFontDatabase::FixedFont)); layout->addWidget(preview);
    auto *confirm = new QCheckBox(i18n::tr("我确认当前终端处于所选 Shell 的空提示符"), &dialog);
    confirm->setObjectName(QStringLiteral("shellReportConfirm")); layout->addWidget(confirm);
    auto *credentials = new QCheckBox(i18n::tr("我确认本次查询可使用 kubeconfig 凭据插件、身份服务和缓存"), &dialog);
    credentials->setObjectName(QStringLiteral("kubeCredentialsConfirm")); layout->addWidget(credentials);
    auto *status = new QLabel(&dialog); status->setWordWrap(true); status->setObjectName(QStringLiteral("shellReportStatus")); layout->addWidget(status);
    auto *path = new QPlainTextEdit(&dialog); path->setReadOnly(true);
    path->setObjectName(QStringLiteral("shellReportPath")); path->setAccessibleName(i18n::tr("Shell 自报结果（仅内存快照）"));
    path->setFont(QFontDatabase::systemFont(QFontDatabase::FixedFont)); layout->addWidget(path);
    auto *buttons = new QDialogButtonBox(QDialogButtonBox::Close, &dialog);
    buttons->button(QDialogButtonBox::Close)->setText(i18n::tr("关闭"));
    auto *refresh = buttons->addButton(i18n::tr("重新预览报告命令"), QDialogButtonBox::ActionRole);
    auto *send = buttons->addButton(i18n::tr("发送报告命令到此会话"), QDialogButtonBox::ActionRole);
    auto *clear = buttons->addButton(i18n::tr("取消采集并清除快照"), QDialogButtonBox::ActionRole);
    refresh->setObjectName(QStringLiteral("shellReportRefresh")); send->setObjectName(QStringLiteral("shellReportSend"));
    clear->setObjectName(QStringLiteral("shellReportClear")); layout->addWidget(buttons);
    auto *apply = new QPushButton(i18n::tr("将解析名字填入草稿（不发送）"), &dialog);
    apply->setObjectName(QStringLiteral("shellLookupApply")); layout->addWidget(apply);
    for (auto *button : buttons->buttons()) if (auto *push = dynamic_cast<QPushButton *>(button)) { push->setAutoDefault(false); push->setDefault(false); }
    apply->setAutoDefault(false); apply->setDefault(false);
    const auto initial = shell_report_poll(tab);
    if (initial.state == 3) {
        mode->setCurrentIndex(initial.name_catalog ? 5 : !initial.kube.config.empty() ? 4 : !initial.docker_context.empty() ? 3 : initial.git ? 2 : initial.query.empty() ? 0 : 1);
        query->setText(q(!initial.kube.config.empty() ? initial.kube.context : initial.docker_context.empty() ? initial.query : initial.docker_context)); syntax->setCurrentIndex(initial.platform == 1 ? 1 : 2);
        config->setText(q(initial.kube.config)); ns->setText(q(initial.kube.namespace_name));
        resource->setCurrentIndex(initial.kube.resource);
        if (initial.name_catalog) query->setText(q(initial.name_prefix));
    }
    QString token, filled;
    auto update = [&] {
        if (!alive || !draft || source() != tab || completion_context(tab).generation != generation) {
            shell_report_clear(tab); dialog.reject(); return;
        }
        auto report = shell_report_poll(tab);
        status->setText(enabled() ? state_text(report) : i18n::tr("请切回文本模式并停止当前发送后再采集。")); token = q(report.token);
        const auto command = q(report.command); if (preview->toPlainText() != command) preview->setPlainText(command);
        if (report.state == 3) {
            const QString name = report.system ? (report.platform==1 ? QStringLiteral("Bash") : QStringLiteral("PowerShell"))+QStringLiteral(" / ")+chrome::completion_system_label(report.system)
                : report.platform == 1 ? QStringLiteral("Bash") : report.platform == 2 ? QStringLiteral("PowerShell / Windows") : QStringLiteral("PowerShell / Unix");
            QString text = report.query.empty() ? name + QLatin1Char('\n') + q(report.path)
                : name + QLatin1Char('\n') + q(report.query) + QStringLiteral(" · ") + category(report.kind) + QLatin1Char('\n') + q(report.location);
            if (report.name_catalog) {
                text = name + QLatin1Char('\n') + i18n::tr("采集前缀：%1").arg(q(report.name_prefix));
                for (const auto &entry : report.names) text += QLatin1Char('\n') + q(entry.name) + QStringLiteral(" · ") + category(entry.kind);
                if (report.truncated) text += QLatin1Char('\n') + i18n::tr("仅保留部分适用名字；可填写更具体的采集前缀后刷新。");
            }
            if (report.git) {
                text = name + QLatin1Char('\n') + q(report.repository);
                for (const auto &ref : report.references) text += QLatin1Char('\n') + q(ref);
                if (report.truncated) text += QLatin1Char('\n') + i18n::tr("仅显示部分 Git 引用，请缩小仓库范围后重新采集。");
            }
            if (!report.docker_context.empty()) {
                text = name + QLatin1Char('\n') + QStringLiteral("context: ") + q(report.docker_context);
                for (const auto &container : report.containers) text += QLatin1Char('\n') + q(container.name) + QStringLiteral(" · ") + q(container.id);
                if (report.truncated) text += QLatin1Char('\n') + i18n::tr("仅显示部分近期 Docker 容器，请重新采集或缩小候选前缀。");
            }
            if (!report.kube.config.empty()) {
                text = name + QLatin1Char('\n') + q(report.kube.context) + QStringLiteral(" / ") + q(report.kube.namespace_name) + QLatin1Char('\n') + q(report.kube.config);
                text += QLatin1Char('\n') + resource->itemText(report.kube.resource);
                for (const auto &entry : report.resources) text += QLatin1Char('\n') + q(entry.name) + QStringLiteral(" · ") + q(entry.uid);
                if (report.truncated) text += QLatin1Char('\n') + i18n::tr("仅保留部分资源；不代表完整 namespace 列表。");
            }
            if (path->toPlainText() != text) path->setPlainText(text);
        } else if (!path->toPlainText().isEmpty()) path->clear();
        const bool kube = mode->currentIndex() == 4;
        const bool names = mode->currentIndex() == 5;
        send->setEnabled(enabled() && report.state == 1 && confirm->isChecked() && (!kube || credentials->isChecked())); refresh->setEnabled(syntax->currentIndex() > 0);
        apply->setVisible(mode->currentIndex() == 1);
        git_hint->setVisible(mode->currentIndex() == 2);
        docker_hint->setVisible(mode->currentIndex() == 3); hint->setVisible(mode->currentIndex() < 3 || names); names_hint->setVisible(names);
        context_label->setText(names ? i18n::tr("采集前缀") : QStringLiteral("context"));
        context_label->setVisible(mode->currentIndex() >= 3); config_label->setVisible(kube); ns_label->setVisible(kube);
        config->setVisible(kube); ns->setVisible(kube); resource->setVisible(kube); kube_hint->setVisible(kube); credentials->setVisible(kube);
        query->setEnabled(mode->currentIndex() == 1 || mode->currentIndex() >= 3);
        query->setMaxLength(mode->currentIndex() == 3 || names ? 64 : 128);
        query->setPlaceholderText(names ? i18n::tr("可选 ASCII 名字前缀；编辑后重新预览，不因打字查询") : kube ? i18n::tr("明确的 kubectl context，最多 128 个受限 ASCII 字符") : mode->currentIndex() == 3 ? i18n::tr("明确的 Docker context 名称；编辑后重新预览") : i18n::tr("命令名，不含路径或参数；编辑后点击重新预览"));
        query->setAccessibleName(query->placeholderText());
        apply->setEnabled(enabled() && draft->text() == original && mode->currentIndex() == 1
            && report.can_fill && q(report.query) == query->text());
    };
    auto prepare = [&] {
        confirm->setChecked(false); credentials->setChecked(false);
        if (mode->currentIndex() == 0) shell_report_prepare(tab, uint8_t(syntax->currentIndex()));
        else if (mode->currentIndex() == 5) { const auto prefix = query->text().toUtf8(); shell_names_prepare(tab, uint8_t(syntax->currentIndex()), rust::Str(prefix.constData(), size_t(prefix.size()))); }
        else if (mode->currentIndex() == 2) shell_git_prepare(tab, uint8_t(syntax->currentIndex()));
        else if (mode->currentIndex() == 3) { const auto context = query->text().toUtf8(); shell_docker_prepare(tab, uint8_t(syntax->currentIndex()), rust::Str(context.constData(), size_t(context.size()))); }
        else if (mode->currentIndex() == 4) {
            KubeScope scope; scope.config = config->text().toUtf8().toStdString(); scope.context = query->text().toUtf8().toStdString(); scope.namespace_name = ns->text().toUtf8().toStdString();
            scope.resource = uint8_t(resource->currentIndex());
            shell_kube_prepare(tab, uint8_t(syntax->currentIndex()), scope);
        }
        else { const auto name = query->text().toUtf8(); shell_resolution_prepare(tab, uint8_t(syntax->currentIndex()), rust::Str(name.constData(), size_t(name.size()))); }
        update();
    };
    QObject::connect(syntax, &QComboBox::currentIndexChanged, &dialog, prepare);
    QObject::connect(mode, &QComboBox::currentIndexChanged, &dialog, [&] { query->clear(); prepare(); });
    auto edited = [&] { shell_report_cancel(tab); confirm->setChecked(false); credentials->setChecked(false); update(); };
    QObject::connect(query, &QLineEdit::textChanged, &dialog, edited);
    QObject::connect(config, &QLineEdit::textChanged, &dialog, edited); QObject::connect(ns, &QLineEdit::textChanged, &dialog, edited);
    QObject::connect(resource, &QComboBox::currentIndexChanged, &dialog, edited);
    QObject::connect(refresh, &QPushButton::clicked, &dialog, prepare);
    QObject::connect(confirm, &QCheckBox::toggled, &dialog, [&] { update(); });
    QObject::connect(credentials, &QCheckBox::toggled, &dialog, [&] { update(); });
    QObject::connect(send, &QPushButton::clicked, &dialog, [&] {
        if (!alive || !draft || source() != tab || completion_context(tab).generation != generation || !enabled() || !confirm->isChecked()) return;
        const auto bytes = token.toUtf8();
        if (mode->currentIndex() == 4) shell_kube_send(tab, rust::Str(bytes.constData(), size_t(bytes.size())), credentials->isChecked());
        else shell_report_send(tab, rust::Str(bytes.constData(), size_t(bytes.size())));
        confirm->setChecked(false); credentials->setChecked(false); update();
    });
    QObject::connect(clear, &QPushButton::clicked, &dialog, [&] { shell_report_clear(tab); confirm->setChecked(false); credentials->setChecked(false); update(); });
    QObject::connect(apply, &QPushButton::clicked, &dialog, [&] {
        if (!alive || !draft || !enabled() || draft->text() != original || source() != tab) return;
        const auto bytes = token.toUtf8(); filled = q(shell_resolution_draft(tab, rust::Str(bytes.constData(), size_t(bytes.size()))));
        if (!filled.isEmpty()) dialog.accept(); else update();
    });
    QObject::connect(buttons, &QDialogButtonBox::rejected, &dialog, &QDialog::reject);
    QTimer timer; timer.setInterval(200); QObject::connect(&timer, &QTimer::timeout, &dialog, update); timer.start(); update();
    dialog.exec();
    // A receipt may arrive after the last 200 ms view refresh. Cancel only a
    // request that is still pending now; preserve a completed backend snapshot.
    const auto closing_state = shell_report_poll(tab).state;
    if (closing_state == 1 || closing_state == 2) shell_report_cancel(tab);
    if (!filled.isEmpty() && alive && draft && enabled() && draft->text() == original && source() == tab
        && completion_context(tab).generation == generation && completion_context(tab).ready) {
        draft->setText(filled); draft->setCursorPosition(filled.size()); draft->setFocus();
    }
}
}
void chrome::install_shell_report(QWidget *owner, QHBoxLayout *row, std::function<uint32_t()> source,
                                 std::function<void()> invalidate, std::function<bool()> enabled, QLineEdit *input) {
    auto *button = new QToolButton(owner); button->setObjectName(QStringLiteral("sendShellReport"));
    button->setText(i18n::tr("Shell 环境…")); button->setToolTip(i18n::tr("预览并显式采集当前终端的命令名字目录、PATH、精确名字、Git 引用、Docker 容器或明确作用域的资源。"));
    button->setAccessibleName(button->toolTip());
    QObject::connect(button, &QToolButton::clicked, owner, [owner, source, invalidate, enabled, input] { invalidate(); show_report(owner, source, enabled, input); });
    row->addWidget(button);
}
