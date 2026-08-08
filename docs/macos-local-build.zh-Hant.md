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

## Release 安裝程式如何處理已簽署資產缺失

不帶 `--local-build` 執行 `./install.sh` 時，會進入 Release 安裝流程。它會
下載目前版本的套件，若 macOS signed 版資產不存在，安裝程式會自動回退至
`-unsigned-preview` 套件，並在繼續前打印**無法跳過的**信任警告。若系統擋下啟動，
請前往「系統設定 → 隱私權與安全性」，對 `~/Applications/AgenTerm.app` 選擇「仍要開啟」。

只有已簽署資產明確回傳 HTTP 404 或 410 時才允許回退。傳輸、認證、rate
limit 或伺服器錯誤都會停止安裝，不會靜默降級。

從原始碼工作目錄安裝時，請使用 `--local-build target/debug`。Release
`-unsigned-preview` 的安裝不需額外設定環境變數，會由安裝程式自動回退並強制警示。
舊命令仍可帶 `AGENTERM_ALLOW_UNSIGNED_PREVIEW=1`，但它現在只是相容性確認，
不會強制選擇預覽版、隱藏警告、略過已簽署資產驗證或改變安裝記錄。

若要安裝最佳化的本機建置：

```bash
./build.sh release-fast
./install.sh --local-build target/release-fast
open ~/Applications/AgenTerm.app
```

本機建置是由使用者機器產生的未簽署內容；Release 的 checksum、簽署、
公證、Candidate 與 Promotion 規則維持不變。
