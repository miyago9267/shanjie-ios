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
            TextField("在這裡試打", text: $text, axis: .vertical)
                .textFieldStyle(.roundedBorder)
                .lineLimit(3...6)
            Spacer()
        }
        .padding(16)
    }
}
