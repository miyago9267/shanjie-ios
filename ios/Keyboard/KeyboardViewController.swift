import UIKit

private enum Act {
    case zhuyin(UInt32), space, enter, backspace, pick(Int), commitAll, openCandidates, closeCandidates
}

/// What the engine queue decided for one tap; applied on main in tap order.
private struct Result {
    var insert = ""
    var deleteBackward = false
    var display: Display?
    var loaded = true
}

final class KeyboardViewController: UIInputViewController {
    // Standard layout (docs/contracts/s3a.md): each key sends its ASCII keycap to the core.
    private static let rows: [[(String, Character)]] = [
        [("ㄅ", "1"), ("ㄉ", "2"), ("ˇ", "3"), ("ˋ", "4"), ("ㄓ", "5"), ("ˊ", "6"), ("˙", "7"), ("ㄚ", "8"), ("ㄞ", "9"), ("ㄢ", "0"), ("ㄦ", "-")],
        [("ㄆ", "q"), ("ㄊ", "w"), ("ㄍ", "e"), ("ㄐ", "r"), ("ㄔ", "t"), ("ㄗ", "y"), ("ㄧ", "u"), ("ㄛ", "i"), ("ㄟ", "o"), ("ㄣ", "p")],
        [("ㄇ", "a"), ("ㄋ", "s"), ("ㄎ", "d"), ("ㄑ", "f"), ("ㄕ", "g"), ("ㄘ", "h"), ("ㄨ", "j"), ("ㄜ", "k"), ("ㄠ", "l"), ("ㄤ", ";")],
        [("ㄈ", "z"), ("ㄌ", "x"), ("ㄏ", "c"), ("ㄒ", "v"), ("ㄖ", "b"), ("ㄙ", "n"), ("ㄩ", "m"), ("ㄝ", ","), ("ㄡ", "."), ("ㄥ", "/")],
    ]

    // Engine-queue state (touched only inside EngineHost.run closures).
    private var composing = false
    private var windowOpen = false

    private let barScroll = UIScrollView()
    private let barStack = UIStackView()
    private let chevron = UIButton(type: .system)
    private var display = Display()
    private var deleteTimer: Timer?

    override func viewDidLoad() {
        super.viewDidLoad()
        buildUI()
        render(Display(), loading: true)
        EngineHost.run({ [weak self] e -> Result in
            self?.composing = false
            self?.windowOpen = false
            _ = e?.reset(commit: false)  // a composition left by an earlier keyboard instance
            return Result(loaded: e != nil)
        }, done: { [weak self] r in self?.render(Display(), loading: !r.loaded) })
    }

    // MARK: engine

    private func send(_ act: Act) {
        let left = display.preedit.isEmpty ? textDocumentProxy.documentContextBeforeInput : nil
        EngineHost.run({ [weak self] e -> Result in
            guard let self, let e else { return Result(loaded: false) }
            return self.handle(act, e, left: left)
        }, done: { [weak self] r in self?.apply(r) })
    }

    private func handle(_ act: Act, _ e: Engine, left: String?) -> Result {
        var r = Result()
        var out: Display?
        switch act {
        case .zhuyin(let ch):
            if !composing, let left { e.setLeftContext(String(left.suffix(8))) }
            out = e.key(1, ch: ch)
        case .space:
            if !composing { r.insert = " " }
            else if pendingSyllable { out = e.key(2) }
            else { out = commitAll(e) }
        case .enter:
            if !composing { r.insert = "\n" } else { out = e.key(3) }
        case .backspace:
            if composing { out = e.key(4) } else { r.deleteBackward = true }
        case .pick(let i):
            out = e.pick(i)
        case .commitAll:
            out = commitAll(e)
        case .openCandidates:
            out = e.key(10)
        case .closeCandidates:
            out = e.key(6)
        }
        if let out {
            r.insert += out.commit
            composing = !out.preedit.isEmpty
            windowOpen = out.selected >= 0
            pendingSyllable = Self.endsWithZhuyin(out.preedit)
            r.display = out
            if !out.commit.isEmpty { Footprint.record("after_commit") }
        } else if case .zhuyin = act {
            // engine failed (it reset itself): drop our view of the composition
            composing = false; windowOpen = false; pendingSyllable = false
            r.display = Display()
        }
        return r
    }

    private var pendingSyllable = false

    private func commitAll(_ e: Engine) -> Display? {
        if windowOpen { _ = e.key(6) }
        return e.reset(commit: true)
    }

    /// Unfinished zhuyin sits at the end of the preedit; a finished syllable has already become Han.
    private static func endsWithZhuyin(_ s: String) -> Bool {
        guard let u = s.unicodeScalars.last?.value else { return false }
        return (0x3105...0x312F).contains(u) || [0x2C7, 0x2CB, 0x2CA, 0x2D9].contains(u)
    }

    private func apply(_ r: Result) {
        if !r.insert.isEmpty { textDocumentProxy.insertText(r.insert) }
        if r.deleteBackward { textDocumentProxy.deleteBackward() }
        if let d = r.display { render(d, loading: false) }
    }

    // MARK: UI

    private func render(_ d: Display, loading: Bool) {
        display = d
        barStack.arrangedSubviews.forEach { $0.removeFromSuperview() }
        if loading { barStack.addArrangedSubview(chip("載入中…", nil)); chevron.isHidden = true; return }
        if !d.preedit.isEmpty { barStack.addArrangedSubview(chip(d.preedit, #selector(commitTapped), bold: true)) }
        for (i, c) in d.candidates.enumerated() {
            let b = chip(c, #selector(candidateTapped(_:)), highlighted: i == d.selected)
            b.tag = i
            barStack.addArrangedSubview(b)
        }
        let open = d.selected >= 0
        chevron.isHidden = d.preedit.isEmpty
        chevron.setTitle(open ? "▴" : "▾", for: .normal)
        barScroll.setContentOffset(.zero, animated: false)
    }

    private func chip(_ title: String, _ action: Selector?, bold: Bool = false, highlighted: Bool = false) -> UIButton {
        var cfg = UIButton.Configuration.plain()
        cfg.contentInsets = NSDirectionalEdgeInsets(top: 0, leading: 10, bottom: 0, trailing: 10)
        cfg.baseForegroundColor = .label
        cfg.attributedTitle = AttributedString(title, attributes: AttributeContainer([
            .font: UIFont.systemFont(ofSize: 20, weight: bold ? .semibold : .regular)]))
        let b = UIButton(configuration: cfg)
        b.backgroundColor = highlighted ? .systemBlue.withAlphaComponent(0.25) : .clear
        b.layer.cornerRadius = 6
        if let action { b.addTarget(self, action: action, for: .touchUpInside) } else { b.isEnabled = false }
        return b
    }

    @objc private func commitTapped() { send(.commitAll) }
    @objc private func candidateTapped(_ b: UIButton) { send(.pick(b.tag)) }
    @objc private func chevronTapped() { send(display.selected >= 0 ? .closeCandidates : .openCandidates) }

    private func buildUI() {
        let root = UIStackView()
        root.axis = .vertical
        root.spacing = 6
        root.translatesAutoresizingMaskIntoConstraints = false
        view.addSubview(root)
        NSLayoutConstraint.activate([
            root.leadingAnchor.constraint(equalTo: view.leadingAnchor, constant: 3),
            root.trailingAnchor.constraint(equalTo: view.trailingAnchor, constant: -3),
            root.topAnchor.constraint(equalTo: view.topAnchor, constant: 4),
            root.bottomAnchor.constraint(equalTo: view.bottomAnchor, constant: -4),
        ])

        barStack.axis = .horizontal
        barStack.spacing = 2
        barStack.translatesAutoresizingMaskIntoConstraints = false
        barScroll.addSubview(barStack)
        barScroll.showsHorizontalScrollIndicator = false
        chevron.titleLabel?.font = .systemFont(ofSize: 20)
        chevron.addTarget(self, action: #selector(chevronTapped), for: .touchUpInside)
        chevron.widthAnchor.constraint(equalToConstant: 44).isActive = true
        let bar = UIStackView(arrangedSubviews: [barScroll, chevron])
        bar.heightAnchor.constraint(equalToConstant: 40).isActive = true
        root.addArrangedSubview(bar)
        NSLayoutConstraint.activate([
            barStack.leadingAnchor.constraint(equalTo: barScroll.contentLayoutGuide.leadingAnchor),
            barStack.trailingAnchor.constraint(equalTo: barScroll.contentLayoutGuide.trailingAnchor),
            barStack.topAnchor.constraint(equalTo: barScroll.contentLayoutGuide.topAnchor),
            barStack.bottomAnchor.constraint(equalTo: barScroll.contentLayoutGuide.bottomAnchor),
            barStack.heightAnchor.constraint(equalTo: barScroll.frameLayoutGuide.heightAnchor),
        ])

        for row in Self.rows {
            let s = UIStackView()
            s.distribution = .fillEqually
            s.spacing = 4
            for (label, ascii) in row {
                let b = key(label)
                b.tag = Int(ascii.unicodeScalars.first!.value)
                b.addTarget(self, action: #selector(zhuyinTapped(_:)), for: .touchDown)
                s.addArrangedSubview(b)
            }
            s.heightAnchor.constraint(equalToConstant: 44).isActive = true
            root.addArrangedSubview(s)
        }

        let globe = key("")
        globe.setImage(UIImage(systemName: "globe"), for: .normal)
        globe.tintColor = .label
        globe.addTarget(self, action: #selector(handleInputModeList(from:with:)), for: .allTouchEvents)
        let space = key("空白")
        space.addTarget(self, action: #selector(spaceTapped), for: .touchDown)
        let back = key("⌫")
        back.addTarget(self, action: #selector(backDown), for: .touchDown)
        back.addTarget(self, action: #selector(backUp), for: [.touchUpInside, .touchUpOutside, .touchCancel])
        let enter = key("換行")
        enter.addTarget(self, action: #selector(enterTapped), for: .touchDown)
        let bottom = UIStackView(arrangedSubviews: [globe, space, back, enter])
        bottom.spacing = 4
        bottom.distribution = .fill
        globe.widthAnchor.constraint(equalToConstant: 56).isActive = true
        back.widthAnchor.constraint(equalToConstant: 64).isActive = true
        enter.widthAnchor.constraint(equalToConstant: 72).isActive = true
        bottom.heightAnchor.constraint(equalToConstant: 44).isActive = true
        root.addArrangedSubview(bottom)
    }

    private func key(_ title: String) -> UIButton {
        let b = UIButton(type: .system)
        b.setTitle(title, for: .normal)
        b.titleLabel?.font = .systemFont(ofSize: 22)
        b.setTitleColor(.label, for: .normal)
        b.backgroundColor = UIColor { $0.userInterfaceStyle == .dark ? UIColor(white: 0.36, alpha: 1) : .white }
        b.layer.cornerRadius = 6
        return b
    }

    @objc private func zhuyinTapped(_ b: UIButton) { send(.zhuyin(UInt32(b.tag))) }
    @objc private func spaceTapped() { send(.space) }
    @objc private func enterTapped() { send(.enter) }
    @objc private func backDown() {
        send(.backspace)
        deleteTimer = Timer.scheduledTimer(withTimeInterval: 0.4, repeats: false) { [weak self] _ in
            self?.deleteTimer = Timer.scheduledTimer(withTimeInterval: 0.08, repeats: true) { _ in self?.send(.backspace) }
        }
    }
    @objc private func backUp() { deleteTimer?.invalidate(); deleteTimer = nil }
}
