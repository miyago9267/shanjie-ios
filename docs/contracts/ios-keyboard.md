# 契約：iOS 鍵盤（ios-keyboard）

狀態：草稿（2026-10-19，等維護者確認）；修訂一（2026-10-19：`Lm` 改成從映射讀、iOS 不放 `digits.tsv`，見 §1 記憶體）已實作。

## 0. 依據

- 維護者 2026-10-19：受不了 Apple 原生注音鍵盤，要把善解移植成 iOS keyboard extension。`docs/PLAN.md` §1 把 iOS 列為 v1 非目標；這份契約是 fork 端的範圍擴張，不改 upstream 的 PLAN。
- 維護者 2026-10-19 選擇題：code 放在這個 fork 的 `ios/`（與 `macos/` 並列，共用 `core/` 與 `data/`）；目標是上 App Store，前期先自簽。
- MVP 功能（維護者）：注音輸入、主題、剪貼簿（手動貼上為主，能讀系統剪貼簿就讀）、快速搜尋、翻譯、可調的佈局、振動與按鍵回饋。

## 1. 已知限制（決定順序的事實）

- **記憶體**：macOS 版重播在沒有 trigram 時 max RSS 約 287 MB（`docs/contracts/trigram-mmap.md` §0）。keyboard extension 超過系統上限就被 jetsam 結束；對策沿用 trigram-mmap：唯讀映射的乾淨頁不算 phys_footprint。
  - **修訂一（2026-10-19，更正）**：commit 13a5740 以為所有模型檔都改成映射了，其實 `Lm::parse` 仍把 `bigram.sjlm`（83,445,569 bytes）的每個陣列（`nxt`、`cnt` 共 9.10M 條邊 × 8 B ≈ 69 MB，加 `off`、`ctx_id`、`uni`、詞表）、`classes.sjc`、`kn.sjkn` 複製進 heap，函式回傳前才放掉映射，等於沒省。實機：iPhone 17 上 extension 約在 177 MiB 被 jetsam 結束（per-process limit，由 rpages 推論，不是直接讀到的上限）；Mac 上重現同樣的載入（`Engine::new` 加 `load_lm`、不載 trigram 與 tinylm、含 `digits.tsv`），phys_footprint 214 MB。
  - 維護者 2026-10-19 選：`Lm` 從映射讀，加上 iOS 不放 `digits.tsv`。`Lm`、`classes.sjc`、`kn.sjkn` 各持有映射，陣列以不對齊的 little-endian 讀取就地讀（檔案格式不變；測試用 `Vec<u8>` 來源照舊）；載入時算出的 `ctx_total`、`ctx_back` 與詞的位置留在 heap（約 7 MB）。`digits.tsv` 缺檔只會讓數字功能關閉（`digits.md` A.3.2；`Engine::new` 用 `.ok()` 讀，FFI 走同一條路），`ios/scripts/copy-resources.sh` 不再複製它。
  - 量測（Mac 複製載入，release，phys_footprint）：修正前 214 MB（不放 `digits.tsv` 約 203 MB）；修正後不放 `digits.tsv` 116 MB（`Engine::new` 後 68 MB，`load_lm` 後 115 MB，打 50 鍵後 116 MB）。dev302 重播的選字與每鍵 p95 沒變。實機 footprint 仍待維護者在 §3 第 4 項量。
- **交叉編譯**：2026-10-19 在 `aarch64-apple-ios` 編 `core` 有 7 個錯誤，都是 `core/src/learn_store.rs` 的 `O_NOFOLLOW`、`O_NONBLOCK`、`ELOOP`、`barrier` 只用 `cfg(target_os = "macos")` 定義；`core/src/tinylm.rs` 也有 macOS 專用的 cfg。iOS 的 `<fcntl.h>` 值與 macOS 相同〔推論，spike 用單元測試核對〕。
- **Full Access（RequestsOpenAccess）**：網路（搜尋、雲端翻譯）、App Group 共用容器、讀 `UIPasteboard.general` 都要它；振動（`UIImpactFeedbackGenerator`）在 keyboard extension 是否也要它〔未確認，實機驗〕。按鍵音用 `UIInputViewAudioFeedback` 與 `playInputClick()`，不需要 Full Access。沒開 Full Access 時打字、選字、主題、佈局、按鍵音都要能用；選字記憶改存 extension 自己的容器。
- **剪貼簿**：extension 沒辦法在背景監聽剪貼簿，歷史只能在鍵盤出現時以 `changeCount` 抓一次；iOS 16 以後是否跳貼上提示〔未確認，實機驗〕。可靠的部分是使用者自己釘選的片段。
- **搜尋**：keyboard extension 不能正規地打開其他 App；結果顯示在鍵盤面板裡，選了就插入連結或文字（與 Gboard 相同的形狀）。
- **翻譯**：Apple Translation framework 能否在 extension 裡用、會吃多少記憶體〔未確認〕；備案是雲端 API。
- **App Store**：Full Access 要在主 App 說明用途、隱私標籤要列出網路功能；網路功能預設關，打字內容不送出裝置，除非使用者對該次搜尋或翻譯按下送出。

## 2. 分片

1. **S-iOS-0 spike（擋住其他片）**：`core` 在 `aarch64-apple-ios`、`aarch64-apple-ios-sim` 編過（cfg 改成 macOS 與 iOS 都用同一組值）；`ios/` 用 XcodeGen（`ios/project.yml`）產生主 App 加 keyboard extension；core 打包成 xcframework；extension 載入 model-v6＋classes-v5＋kn-v1，不載 trigram 與 tinylm；能打出一句注音並選字送出。所有模型改成唯讀映射（`lm.rs` 的讀檔改用 trigram-mmap 的映射型別），選字輸出逐位元組不變。
2. **S-iOS-1 打字 MVP**：標準注音排列、候選列（橫向捲動，展開成網格）、標點、空白與 Enter、Backspace 逐符號刪；選字記憶；主 App 的設定頁。
3. **S-iOS-2 外觀與手感**：主題（淺、深、跟系統、自訂顏色與背景圖）；佈局（倚天排列、鍵高、數字列開關、單手模式）；振動（開關、強度）與按鍵音（跟系統設定）。
4. **S-iOS-3 剪貼簿**：釘選片段（App Group）、鍵盤出現時抓到的最近項目、一鍵插入。
5. **S-iOS-4 搜尋與翻譯**：需要 Full Access，預設關；搜尋 API 與翻譯後端另寫契約。

## 3. S-iOS-0 驗收

1. `cargo test -p core` 在 Mac 照舊全過；新增的單元測試核對 iOS 與 macOS 的 fcntl 常數相同。
2. 映射改動：`cargo test -p cli --test golden` 不改任何 golden 檔全過；重播（`tools/bench/replay`，dev302）的選字輸出與 main 逐列相同，peak memory footprint 比 main 少（數字記進研究紀錄）。
3. 模擬器：`xcodebuild` 建置通過，模擬器上切到善解鍵盤打「ㄋㄧˇ ㄏㄠˇ」能選出「你好」送出。
4. 實機（維護者的 iPhone，自簽）：同一句打得出來；在 Xcode 的 Memory Report 記下 extension 的 footprint（剛出現、打完一句、連打 50 鍵後）。

## 4. 停止條件

- 實機 footprint 在映射後仍被系統結束 → 停，回報數字，問維護者（選項：iOS 用較小的 bigram、拿掉 kn、或另訓 iOS 專用模型）。
- 映射改動讓任何選字輸出改變 → 停。
- 實機需要的簽章或帳號設定只有維護者能做 → 交給維護者，其他步驟不等。
