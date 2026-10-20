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

/// Input view that opts in to the system keyboard click sound.
private final class ClickInputView: UIInputView, UIInputViewAudioFeedback {
    var enableInputClicksWhenVisible: Bool { true }
}

/// Lays subviews out left to right and hides those that do not fit; no scrolling.
private final class FitRow: UIView {
    var gap: CGFloat = 2
    override func layoutSubviews() {
        super.layoutSubviews()
        var x: CGFloat = 0
        for v in subviews {
            let w = v.systemLayoutSizeFitting(UIView.layoutFittingCompressedSize).width
            if x + w <= bounds.width { v.frame = CGRect(x: x, y: 0, width: w, height: bounds.height); v.isHidden = false; x += w + gap }
            else { v.isHidden = true }
        }
    }
    func relayout() { setNeedsLayout() }
}

/// Layout constants for the keyboard; one place for a later settings slice.
enum KeyMetrics {
    static var keyHeight: CGFloat = 48
    static var rowSpacing: CGFloat = 8
    static var keySpacing: CGFloat = 6
    static var cornerRadius: CGFloat = 9
    static var labelSize: CGFloat = 24
    static var sideInset: CGFloat = 3
}

private final class Key: UIButton {
    var glyph = ""
}

/// Places keys left to right in `units` of one key width (spacing included between keys).
/// `inset` is in key units; hidden keys are skipped; the flexible key takes the remaining units.
private final class KeyRow: UIView {
    var items: [(view: UIView, units: CGFloat)] = []
    var flexible: UIView?
    var inset: CGFloat = 0
    var total: CGFloat = 11

    func add(_ v: UIView, units: CGFloat = 1) { items.append((v, units)); addSubview(v) }

    override func layoutSubviews() {
        super.layoutSubviews()
        let gap = KeyMetrics.keySpacing
        let u = (bounds.width - gap * (total - 1)) / total   // width of one key unit
        let shown = items.filter { !$0.view.isHidden }
        let fixed = shown.filter { $0.view !== flexible }.reduce(0) { $0 + $1.units }
        let flexUnits = max(total - inset - fixed, 1)
        var x = inset * (u + gap)
        for it in shown {
            let n = it.view === flexible ? flexUnits : it.units
            let w = n * u + (n - 1) * gap
            it.view.frame = CGRect(x: x, y: 0, width: w, height: bounds.height)
            x += w + gap
        }
    }
}

/// One reusable pop-up showing the enlarged glyph of the held key.
private final class Callout: UILabel {
    override init(frame: CGRect) {
        super.init(frame: frame)
        font = .systemFont(ofSize: 34)
        textAlignment = .center
        textColor = .label
        backgroundColor = UIColor { $0.userInterfaceStyle == .dark ? UIColor(white: 0.42, alpha: 1) : .white }
        layer.cornerRadius = 8
        layer.masksToBounds = false
        layer.shadowColor = UIColor.black.cgColor
        layer.shadowOpacity = 0.3
        layer.shadowRadius = 3
        layer.shadowOffset = CGSize(width: 0, height: 1)
        isHidden = true
    }
    required init?(coder: NSCoder) { fatalError() }
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

    private let barRow = FitRow()
    private let callout = Callout()
    private let haptic = UIImpactFeedbackGenerator(style: .light)
    private let chevron = UIButton(type: .system)
    private lazy var globe = key("")
    private lazy var symbols = key("123")
    private lazy var modeKey = key("")
    private lazy var space = key("")
    private lazy var enter = key("↵")
    private let spaceHint = UILabel()
    private var bottomRow: KeyRow?
    private var display = Display()
    private var deleteTimer: Timer?

    override func loadView() {
        inputView = ClickInputView(frame: .zero, inputViewStyle: .keyboard)
    }

    override func viewWillAppear(_ animated: Bool) {
        super.viewWillAppear(animated)
        if hasFullAccess { haptic.prepare() }
    }

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
        barRow.subviews.forEach { $0.removeFromSuperview() }
        if loading { barRow.addSubview(chip("載入中…", nil)); chevron.isHidden = true; barRow.relayout(); return }
        if !d.preedit.isEmpty { barRow.addSubview(chip(d.preedit, #selector(commitTapped), bold: true)) }
        for (i, c) in d.candidates.enumerated() {
            let b = chip(c, #selector(candidateTapped(_:)), highlighted: i == d.selected)
            b.tag = i
            barRow.addSubview(b)
        }
        let open = d.selected >= 0
        chevron.isHidden = d.preedit.isEmpty
        chevron.setTitle(open ? "▴" : "▾", for: .normal)
        barRow.relayout()
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

    override func viewWillLayoutSubviews() {
        super.viewWillLayoutSubviews()
        if globe.isHidden == needsInputModeSwitchKey {
            globe.isHidden = !needsInputModeSwitchKey
            bottomRow?.setNeedsLayout()
        }
    }

    private func buildUI() {
        let root = UIStackView()
        root.axis = .vertical
        root.spacing = KeyMetrics.rowSpacing
        root.translatesAutoresizingMaskIntoConstraints = false
        view.addSubview(root)
        NSLayoutConstraint.activate([
            root.leadingAnchor.constraint(equalTo: view.leadingAnchor, constant: KeyMetrics.sideInset),
            root.trailingAnchor.constraint(equalTo: view.trailingAnchor, constant: -KeyMetrics.sideInset),
            root.topAnchor.constraint(equalTo: view.topAnchor, constant: 8),
            root.bottomAnchor.constraint(equalTo: view.bottomAnchor, constant: -4),
        ])

        chevron.titleLabel?.font = .systemFont(ofSize: 20)
        chevron.addTarget(self, action: #selector(chevronTapped), for: .touchUpInside)
        chevron.widthAnchor.constraint(equalToConstant: 44).isActive = true
        let bar = UIStackView(arrangedSubviews: [barRow, chevron])
        bar.heightAnchor.constraint(equalToConstant: 40).isActive = true
        root.addArrangedSubview(bar)

        let insets: [CGFloat] = [0, 0.5, 1, 0]
        for (r, row) in Self.rows.enumerated() {
            let s = KeyRow()
            s.inset = insets[r]
            s.total = r == 1 ? 10 + 1 : 11
            for (label, ascii) in row {
                let b = key(label)
                b.tag = Int(ascii.unicodeScalars.first!.value)
                b.addTarget(self, action: #selector(zhuyinTapped(_:)), for: .touchDown)
                b.addTarget(self, action: #selector(showCallout(_:)), for: .touchDown)
                s.add(b)
            }
            if r == 3 {
                let back = key("⌫")
                back.addTarget(self, action: #selector(backDown), for: .touchDown)
                back.addTarget(self, action: #selector(backUp), for: [.touchUpInside, .touchUpOutside, .touchCancel])
                back.addTarget(self, action: #selector(feedback), for: .touchDown)
                s.add(back)
            }
            s.heightAnchor.constraint(equalToConstant: KeyMetrics.keyHeight).isActive = true
            root.addArrangedSubview(s)
        }

        globe.setImage(UIImage(systemName: "globe"), for: .normal)
        globe.addTarget(self, action: #selector(handleInputModeList(from:with:)), for: .allTouchEvents)
        symbols.addTarget(self, action: #selector(symbolsTapped), for: .touchDown)
        modeKey.addTarget(self, action: #selector(modeTapped), for: .touchDown)
        space.addTarget(self, action: #selector(spaceTapped), for: .touchDown)
        enter.addTarget(self, action: #selector(enterTapped), for: .touchDown)
        for v in [globe, symbols, modeKey, space, enter] { v.addTarget(self, action: #selector(feedback), for: .touchDown) }

        spaceHint.font = .systemFont(ofSize: 12)
        spaceHint.textColor = .secondaryLabel
        spaceHint.text = mode.hint
        spaceHint.translatesAutoresizingMaskIntoConstraints = false
        space.addSubview(spaceHint)
        NSLayoutConstraint.activate([
            spaceHint.trailingAnchor.constraint(equalTo: space.trailingAnchor, constant: -8),
            spaceHint.bottomAnchor.constraint(equalTo: space.bottomAnchor, constant: -4),
        ])
        modeKey.setImage(UIImage(systemName: "character.bubble"), for: .normal)

        let bottom = KeyRow()
        bottom.add(symbols, units: 1.5)
        bottom.add(modeKey, units: 1.5)
        bottom.add(globe, units: 1.5)
        bottom.add(space)
        bottom.add(enter, units: 2.5)
        bottom.flexible = space
        bottom.heightAnchor.constraint(equalToConstant: KeyMetrics.keyHeight).isActive = true
        self.bottomRow = bottom
        root.addArrangedSubview(bottom)
        view.addSubview(callout)
    }

    private enum InputMode: CaseIterable {
        case zhuyin, english
        var name: String { self == .zhuyin ? "注音" : "英文" }
        var hint: String { self == .zhuyin ? "注" : "英" }
    }
    private var mode = InputMode.zhuyin
    private var hintReset: DispatchWorkItem?

    // Placeholder: UI state only, the engine is not told.
    @objc private func modeTapped() {
        let all = InputMode.allCases
        mode = all[(all.firstIndex(of: mode)! + 1) % all.count]
        spaceHint.text = mode.name
        hintReset?.cancel()
        let w = DispatchWorkItem { [weak self] in self?.spaceHint.text = self?.mode.hint }
        hintReset = w
        DispatchQueue.main.asyncAfter(deadline: .now() + 1.2, execute: w)
    }

    @objc private func symbolsTapped() {}

    @objc private func feedback() {
        UIDevice.current.playInputClick()
        if hasFullAccess { haptic.impactOccurred(); haptic.prepare() }
    }

    @objc private func showCallout(_ b: Key) {
        callout.text = b.glyph
        let f = b.convert(b.bounds, to: view)
        let w = f.width + 16, h: CGFloat = 56
        callout.frame = CGRect(x: min(max(f.midX - w / 2, 0), view.bounds.width - w), y: max(f.minY - h - 4, 0), width: w, height: h)
        callout.isHidden = false
    }

    @objc private func hideCallout() { callout.isHidden = true }

    private var glass: Bool { if #available(iOS 26, *) { return true } else { return false } }

    private func key(_ title: String) -> Key {
        let b = Key()
        b.glyph = title
        var cfg: UIButton.Configuration
        if #available(iOS 26, *) { cfg = .glass() } else { cfg = .plain(); b.backgroundColor = Self.keyBg }
        cfg.cornerStyle = .fixed
        cfg.background.cornerRadius = KeyMetrics.cornerRadius
        cfg.contentInsets = .zero
        cfg.baseForegroundColor = .label
        cfg.preferredSymbolConfigurationForImage = UIImage.SymbolConfiguration(pointSize: KeyMetrics.labelSize - 4)
        cfg.attributedTitle = AttributedString(title, attributes: AttributeContainer([
            .font: UIFont.systemFont(ofSize: KeyMetrics.labelSize)]))
        b.configuration = cfg
        b.layer.cornerCurve = .continuous
        b.layer.cornerRadius = KeyMetrics.cornerRadius
        b.addTarget(self, action: #selector(pressDown(_:)), for: .touchDown)
        b.addTarget(self, action: #selector(pressUp(_:)), for: [.touchUpInside, .touchUpOutside, .touchCancel, .touchDragExit])
        return b
    }

    private static let keyBg = UIColor { $0.userInterfaceStyle == .dark ? UIColor(white: 0.36, alpha: 1) : .white }
    private static let keyBgPressed = UIColor { $0.userInterfaceStyle == .dark ? UIColor(white: 0.55, alpha: 1) : UIColor(white: 0.78, alpha: 1) }

    @objc private func pressDown(_ b: UIButton) { if !glass { b.backgroundColor = Self.keyBgPressed } }
    @objc private func pressUp(_ b: UIButton) { if !glass { b.backgroundColor = Self.keyBg }; hideCallout() }

    @objc private func zhuyinTapped(_ b: UIButton) { feedback(); send(.zhuyin(UInt32(b.tag))) }
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
