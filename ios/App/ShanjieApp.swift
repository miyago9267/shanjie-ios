import SwiftUI

@main
struct ShanjieApp: App {
    var body: some Scene {
        WindowGroup { ContentView() }
    }
}

struct ContentView: View {
    @State private var text = ""

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("善解").font(.largeTitle).bold()
            Text("""
            啟用鍵盤：
            1. 設定 > 一般 > 鍵盤 > 鍵盤 > 新增鍵盤
            2. 選「善解」
            3. 在下面的輸入框長按地球鍵，切到「善解」
            """)
            Text("""
            完整取用：
            鍵盤需要「允許完整取用」才能提供按鍵震動回饋；之後的剪貼簿與搜尋功能也會用到。你打的字不會傳送到任何地方，一律只在手機上處理。
            開啟位置：設定 > 一般 > 鍵盤 > 鍵盤 > 善解 > 允許完整取用
            """)
            .font(.footnote)
            .foregroundStyle(.secondary)
            TextField("在這裡試打", text: $text, axis: .vertical)
                .textFieldStyle(.roundedBorder)
                .lineLimit(3...6)
            Spacer()
        }
        .padding(16)
    }
}
