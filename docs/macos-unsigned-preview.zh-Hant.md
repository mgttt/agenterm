# AgenTerm macOS 未簽署預覽版

產品歸屬：
[交付與品質](../prd/PRD_02_17_delivery_quality.md)。
English: [macOS unsigned preview](macos-unsigned-preview.md).

此封存檔是**未簽署、未經公證的開發者預覽版**，並非 macOS 穩定發行
管道。Apple 尚未驗證發行者，也未透過 Apple Notary Service 掃描此版本。

只有在你瞭解「略過 macOS 對未驗證軟體的保護會帶來額外風險」時，才應
使用此預覽版。

只有已簽署的 macOS 資產返回 HTTP `404` 或 `410` 時，`install.sh` 才會
自動回退至另行命名的 `-unsigned-preview` 封存檔。其他下載錯誤，包括
傳輸、認證、rate limit 或伺服器錯誤，一律 fail-closed。下載預覽版前，
安裝程式一定會將信任警告輸出至 stderr，且沒有可隱藏警告的選項。

對於已簽署發行版，安裝程式要求 provenance 明確記錄
`channel=release`、`signed=true` 與 `notarized=true`。它也會在本機對每個
必要執行檔執行嚴格的 Apple 程式碼簽章校驗，並要求各簽章識別為 Apple
Developer ID Application 權威。因此，已簽署發行版的信任結論是由
provenance 聲明與本機執行檔校驗共同推斷，而不是只依賴其中一項訊號。

`AGENTERM_ALLOW_UNSIGNED_PREVIEW=1` 僅保留為舊安裝命令的相容性確認；它
不會強制選擇未簽署內容、略過簽章驗證、隱藏警告或改變安裝記錄。
它也不會略過 provenance 或本機 Apple 簽章校驗。

## 請先驗證下載內容

請從同一個 GitHub Release 下載符合你電腦架構的三個檔案：

```text
agenterm-…-macos-…-unsigned-preview.zip
agenterm-…-macos-…-unsigned-preview.zip.sha256
agenterm-…-macos-…-unsigned-preview.zip.provenance.json
```

在「終端機」中切換至下載目錄，然後執行：

```sh
shasum -a 256 -c agenterm-*-macos-*-unsigned-preview.zip.sha256
```

結果必須顯示 `OK`。Provenance JSON 會記錄確切的 Git tag、來源 commit、
架構、封存檔 SHA-256、`Cargo.lock` 雜湊、artifact manifest 雜湊，以及
GitHub Actions 建置記錄 URL。同一個 Release 也會提供
`agenterm-…-sbom.spdx.json`，也就是依鎖定版本來源產生的相依套件清單。

安裝程式會自動執行相同的 SHA-256 與 provenance 核對，並要求此預覽版的
provenance 明確記錄 `channel=macos-unsigned-preview`、`signed=false` 與
`notarized=false`。

## 開啟預覽版

1. 解壓縮 ZIP。
2. 先嘗試開啟一次 `agenterm`。macOS 應會阻擋它，並在安全性設定中記錄
   這次開啟嘗試。
3. 開啟 **Apple 選單 → 系統設定 → 隱私權與安全性**。
4. 捲動至**安全性**，找到關於 `agenterm` 的訊息，然後選擇
   **仍要打開**。
5. macOS 要求時進行認證，接著確認**打開**。

遭阻擋的開啟嘗試發生後，Apple 通常只會讓**仍要打開**選項顯示約一小時。
請參閱 Apple 目前的操作說明：
<https://support.apple.com/guide/mac-help/mh40617/mac>。

封存檔內含數個執行檔。第一次執行個別 CLI 執行檔時，macOS 可能會要求
你分別明確核准。

## 請勿停用整個系統的保護

請勿使用 `spctl --master-disable`、請勿在整個系統停用 Gatekeeper，也
不要遞迴移除其他不相關檔案的 quarantine 屬性。此預覽版刻意採用 macOS
針對個別 App、由使用者明確核准的發行路徑。

如果 macOS 顯示軟體**將損害你的電腦**、軟體已被移至「垃圾桶」，或軟體
看起來遭到修改，而不只是無法識別開發者，請勿略過警告。刪除下載內容，
並回報完整訊息。

## 回報問題

請附上：

- macOS 版本與 Mac 架構；
- 封存檔名稱與 SHA-256；
- Provenance JSON 中的 `source_commit`；
- 完整的 Gatekeeper 訊息或終端機輸出；
- 發生問題的是 GUI 或哪一個 CLI 執行檔。

請勿附上密碼、token、Proxy 認證資訊、終端內容或其他機密資料。
