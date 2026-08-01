# 在 macOS 本機建置與安裝 AgenTerm

English: [Build and install AgenTerm locally on macOS](macos-local-build.md).

從原始碼工作目錄執行 AgenTerm 時，請使用本機建置安裝路徑。它會建立真正
的 `~/Applications/AgenTerm.app`；請勿把裸執行檔
`target/debug/agenterm` 保留在 Dock。

```bash
./build.sh
./install.sh --local-build target/debug
open ~/Applications/AgenTerm.app
```

App 開啟後，請在 Dock 保留 **AgenTerm.app**。安裝程式也會把本機建置複製
至 `~/.local/share/agenterm` 下的版本目錄，並更新 `~/.local/bin` 中的命令。

## 為何直接執行 `./install.sh` 可能出現 404

不帶 `--local-build` 執行 `./install.sh` 時，會進入 Release 安裝流程。它會
下載目前版本的套件，而且穩定 macOS 管道要求已簽署資產。若該 Release
尚未發布對應資產，安裝程式會回報 HTTP 404，並在不修改現有安裝的情況下
結束。

從原始碼工作目錄安裝時，請使用 `--local-build target/debug`；不要設定
`AGENTERM_ALLOW_UNSIGNED_PREVIEW=1`。該環境變數只用於明確選擇已發布的
未簽署預覽封存檔，自己的本機建置不需要它。

若要安裝最佳化的本機建置：

```bash
./build.sh release-fast
./install.sh --local-build target/release-fast
open ~/Applications/AgenTerm.app
```

本機建置是由使用者機器產生的未簽署內容；Release 的 checksum、簽署、
公證、Candidate 與 Promotion 規則維持不變。
