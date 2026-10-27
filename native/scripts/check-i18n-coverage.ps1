# Exact direct-literal resource contract; dynamic calls are reported separately.
[CmdletBinding()]
param(
    [string]$SourceRoot = (Join-Path (Split-Path -Parent $PSScriptRoot) 'cpp'),
    [string]$TranslationPath = (Join-Path (Split-Path -Parent $PSScriptRoot) 'i18n/tideterm_en.ts')
)
$ErrorActionPreference = 'Stop'
# Keys retain Unicode, punctuation and whitespace, as i18n.cpp::parseTs does.
$xml = [xml]::new()
$xml.PreserveWhitespace = $true
$xml.Load([IO.Path]::GetFullPath($TranslationPath))
$translations = [Collections.Generic.Dictionary[string,string]]::new([StringComparer]::Ordinal)
$failures = 0
function Get-Placeholders([string]$Text) {
    (@([regex]::Matches($Text, '%(?:L?\d+|n)') | ForEach-Object Value | Sort-Object) -join '|')
}
foreach ($message in $xml.SelectNodes('//message')) {
    $source = $message.SelectSingleNode('source')
    $translation = $message.SelectSingleNode('translation')
    if (-not $source -or -not $translation -or $source.InnerText.Length -eq 0 -or
        $translation.InnerText.Length -eq 0 -or $translation.GetAttribute('type') -in @('unfinished','vanished')) { continue }
    $key = $source.InnerText; $value = $translation.InnerText
    if ($translations.ContainsKey($key) -and $translations[$key] -cne $value) {
        Write-Output "CONFLICT: [$key]"; $failures++
    }
    if ((Get-Placeholders $key) -cne (Get-Placeholders $value)) {
        Write-Output "PLACEHOLDERS: [$key] -> [$value]"; $failures++
    }
    $translations[$key] = $value
}
function Convert-CppLiteral([string]$Token) {
    $body = $Token.Substring(1, $Token.Length - 2)
    $bytes = [Collections.Generic.List[byte]]::new()
    $utf8 = [Text.UTF8Encoding]::new($false, $true)
    for ($i = 0; $i -lt $body.Length; $i++) {
        $c = $body[$i]
        if ($c -ne '\') {
            $part = [string]$c
            if ([char]::IsHighSurrogate($c)) { $i++; $part += $body[$i] }
            $bytes.AddRange($utf8.GetBytes($part)); continue
        }
        $i++; $escape = $body[$i]
        $simple = @{ 'a'=7; 'b'=8; 'f'=12; 'n'=10; 'r'=13; 't'=9; 'v'=11; '\'=92; '"'=34; "'"=39; '?'=63 }
        if ($simple.ContainsKey([string]$escape)) { $bytes.Add([byte]$simple[[string]$escape]); continue }
        if ($escape -eq "`n") { continue }
        if ($escape -eq "`r" -and $body[$i+1] -eq "`n") { $i++; continue }
        $rest = $body.Substring($i)
        if ($rest -match '^x([0-9a-fA-F]+)') {
            $bytes.Add([byte][Convert]::ToInt32($Matches[1],16)); $i += $Matches[0].Length-1
        } elseif ($rest -match '^[0-7]{1,3}') {
            $bytes.Add([byte][Convert]::ToInt32($Matches[0],8)); $i += $Matches[0].Length-1
        } elseif ($rest -match '^u([0-9a-fA-F]{4})|^U([0-9a-fA-F]{8})') {
            $hex = if ($Matches[1]) { $Matches[1] } else { $Matches[2] }
            $bytes.AddRange($utf8.GetBytes([char]::ConvertFromUtf32([Convert]::ToInt32($hex,16))))
            $i += $Matches[0].Length-1
        } else { throw "Unsupported C++ escape in $Token" }
    }
    $utf8.GetString($bytes.ToArray())
}
# Lex comments and character/string literals first so comment text cannot create calls.
$lexer = [regex]::new('(?<comment>//(?:\\\r?\n|[^\r\n])*|/\*[\s\S]*?\*/)|(?<raw>(?:u8)?R"(?<delimiter>[^\s()\\]{0,16})\([\s\S]*?\)\k<delimiter>")|(?<string>"(?:\\[\s\S]|[^"\\])*")|(?<char>''(?:\\[\s\S]|[^''\\])*'')|(?<word>[A-Za-z_]\w*)|(?<punct>::|[^\s])')
$direct = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
$dynamic = 0; $allowed = 0
foreach ($file in Get-ChildItem -LiteralPath $SourceRoot -Recurse -File | Where-Object { $_.Extension -in @('.cpp','.h') }) {
    $text = [IO.File]::ReadAllText($file.FullName)
    $tokens = @($lexer.Matches($text) | Where-Object { -not $_.Groups['comment'].Success })
    for ($i = 0; $i + 4 -lt $tokens.Count; $i++) {
        if ($tokens[$i].Value -cne 'i18n' -or $tokens[$i+1].Value -ne '::' -or
            $tokens[$i+2].Value -cne 'tr' -or $tokens[$i+3].Value -ne '(') { continue }
        $line = 1 + [regex]::Matches($text.Substring(0,$tokens[$i].Index), "`n").Count
        $location = "$($file.FullName):$line"
        $j = $i+4; $key = ''
        while ($j -lt $tokens.Count -and ($tokens[$j].Groups['string'].Success -or $tokens[$j].Groups['raw'].Success)) {
            if ($tokens[$j].Groups['raw'].Success) {
                $rawToken = $tokens[$j].Value
                $start = $rawToken.IndexOf('(')+1
                $key += $rawToken.Substring($start,$rawToken.LastIndexOf(')')-$start)
            } else { $key += Convert-CppLiteral $tokens[$j].Value }
            $j++
        }
        if ($j -eq $i+4 -or $tokens[$j].Value -ne ')') { Write-Output "DYNAMIC: $location"; $dynamic++; continue }
        if ($key -notmatch '[\p{IsCJKUnifiedIdeographs}\p{IsCJKUnifiedIdeographsExtensionA}]') { continue }
        [void]$direct.Add($key)
        if (-not $translations.ContainsKey($key)) {
            # One intentional runtime fallback fixture, identified by exact path AND key.
            $relative = [IO.Path]::GetRelativePath([IO.Path]::GetFullPath($SourceRoot),$file.FullName).Replace('\','/')
            if ($relative -ceq 'i18n_selftest.cpp' -and $key -ceq '一条没登记过的中文') { $allowed++; continue }
            Write-Output "MISSING: $location [$key]"; $failures++
        }
    }
}
Write-Output "i18n: effective=$($translations.Count) direct-Chinese=$($direct.Count) fallback-fixtures=$allowed dynamic=$dynamic failures=$failures"
if ($failures) { exit 1 }
exit 0
