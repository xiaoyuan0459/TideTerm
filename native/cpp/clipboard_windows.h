#pragma once
#include <QString>
#include <qt_windows.h>
#include <cstring>
#include <algorithm>

namespace host_clipboard {
class Access final {
public:
    explicit Access(HWND owner) {
        const ULONGLONG started = GetTickCount64();
        do {
            if (OpenClipboard(owner)) { opened_ = true; return; }
            if (GetLastError() != ERROR_ACCESS_DENIED) return;
            Sleep(1);
        } while (GetTickCount64() - started < 50);
    }
    ~Access() { if (opened_) CloseClipboard(); }
    Access(const Access &) = delete;
    Access &operator=(const Access &) = delete;
    explicit operator bool() const { return opened_; }
private:
    bool opened_ = false;
};

// Publish fully rendered Unicode text. Readers must not depend on a live OLE
// data object or a callback into our GUI thread to obtain this small payload.
inline bool publish_unicode(HWND owner, const QString &text) {
    if (!owner) return false;
    QString native = text;
    native.replace(QLatin1String("\r\n"), QLatin1String("\n"));
    native.replace(QLatin1String("\n"), QLatin1String("\r\n"));
    const SIZE_T bytes = (SIZE_T(native.size()) + 1) * sizeof(wchar_t);
    HGLOBAL data = GlobalAlloc(GMEM_MOVEABLE, bytes);
    if (!data) return false;
    auto *destination = static_cast<wchar_t *>(GlobalLock(data));
    if (!destination) { GlobalFree(data); return false; }
    std::memcpy(destination, native.utf16(), size_t(native.size()) * sizeof(wchar_t));
    destination[native.size()] = L'\0';
    GlobalUnlock(data);

    // Only acquiring access is retried; one operation publishes its text once.
    const Access access(owner);
    if (!access) { GlobalFree(data); return false; }
    const bool copied = EmptyClipboard() && SetClipboardData(CF_UNICODETEXT, data);
    if (!copied) GlobalFree(data); // Windows owns the allocation after success.
    return copied;
}

inline QString read_unicode() {
    const Access access(nullptr);
    if (!access) return {};
    const HANDLE data = GetClipboardData(CF_UNICODETEXT);
    if (!data) return {};
    const auto *text = static_cast<const wchar_t *>(GlobalLock(data));
    if (!text) return {};
    const SIZE_T capacity = GlobalSize(data) / sizeof(wchar_t);
    const auto *end = std::find(text, text + capacity, L'\0');
    QString value;
    if (end != text + capacity) value = QString::fromWCharArray(text, qsizetype(end - text));
    GlobalUnlock(data);
    return value.replace(QLatin1String("\r\n"), QLatin1String("\n"));
}
}
