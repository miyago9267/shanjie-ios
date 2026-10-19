import CShanjie
import Foundation
import os

/// One engine reply, copied out of the C output (shanjie.h: output text is user input, never logged).
struct Display {
    var handled = false
    var commit = ""
    var preedit = ""
    var candidates: [String] = []
    var selected = -1
}

/// Wraps one ShanjieEngine. The handle is not thread-safe: every call goes through `EngineHost.queue`.
final class Engine {
    private var h: OpaquePointer?

    init?(dataDir: String) {
        guard shanjie_engine_new(dataDir, 0, &h) == 0, h != nil else { return nil }
    }

    deinit { shanjie_engine_free(h) }

    func loadLM(_ path: String) -> Int32 { shanjie_engine_load_lm(h, path) }

    func setProfile(_ p: UInt32) {
        var out: UnsafeMutablePointer<ShanjieOutput>?
        _ = copy(shanjie_engine_set_profile(h, p, &out), out)
    }

    func setLeftContext(_ s: String) { _ = shanjie_engine_set_left_context(h, s) }

    func key(_ kind: UInt32, ch: UInt32 = 0) -> Display? {
        var out: UnsafeMutablePointer<ShanjieOutput>?
        return copy(shanjie_engine_key(h, ShanjieKey(kind: kind, ch: ch, modifiers: 0), &out), out)
    }

    func pick(_ i: Int) -> Display? {
        var out: UnsafeMutablePointer<ShanjieOutput>?
        return copy(shanjie_engine_pick(h, UInt32(i), &out), out)
    }

    func reset(commit: Bool) -> Display? {
        var out: UnsafeMutablePointer<ShanjieOutput>?
        return copy(shanjie_engine_reset(h, commit ? 0 : 1, &out), out)
    }

    /// Copies every field into Swift values and frees the C output at once; nil on a non-zero code.
    private func copy(_ code: Int32, _ out: UnsafeMutablePointer<ShanjieOutput>?) -> Display? {
        guard code == 0, let o = out else { return nil }
        defer { shanjie_output_free(o) }
        let p = o.pointee
        var cands: [String] = []
        if let list = p.candidates { for i in 0..<Int(p.candidate_count) { if let c = list[i] { cands.append(String(cString: c)) } } }
        return Display(handled: p.handled != 0, commit: String(cString: p.commit), preedit: String(cString: p.preedit),
                       candidates: cands, selected: Int(p.candidate_selected))
    }
}

enum Footprint {
    private static let log = Logger(subsystem: "com.miyago.shanjie.keyboard", category: "footprint")

    /// phys_footprint (what jetsam counts) and the remaining budget, in MB. Numbers only, no input text.
    static func record(_ label: String) {
        var info = task_vm_info_data_t()
        var count = mach_msg_type_number_t(MemoryLayout<task_vm_info_data_t>.size / MemoryLayout<integer_t>.size)
        let kr = withUnsafeMutablePointer(to: &info) {
            $0.withMemoryRebound(to: integer_t.self, capacity: Int(count)) {
                task_info(mach_task_self_, task_flavor_t(TASK_VM_INFO), $0, &count)
            }
        }
        let mb = kr == KERN_SUCCESS ? Double(info.phys_footprint) / 1_048_576 : -1
        let avail = Double(os_proc_available_memory()) / 1_048_576
        log.notice("footprint \(label, privacy: .public) phys_footprint_mb=\(mb, format: .fixed(precision: 1), privacy: .public) available_mb=\(avail, format: .fixed(precision: 1), privacy: .public)")
    }
}

/// The one engine of the extension process, created and used on a background serial queue.
enum EngineHost {
    static let queue = DispatchQueue(label: "com.miyago.shanjie.engine", qos: .userInitiated)
    private static var engine: Engine?
    private static var started = false
    private static let log = Logger(subsystem: "com.miyago.shanjie.keyboard", category: "engine")

    /// Runs `work` on the engine queue with the engine, loading it first on the first call. `done` runs on main.
    static func run<T>(_ work: @escaping (Engine?) -> T, done: @escaping (T) -> Void) {
        queue.async {
            if !started {
                started = true
                load()
            }
            let r = work(engine)
            DispatchQueue.main.async { done(r) }
        }
    }

    private static func load() {
        let dir = Bundle.main.resourcePath ?? ""
        let t0 = Date()
        guard let e = Engine(dataDir: dir) else { log.error("shanjie_engine_new failed"); return }
        let code = e.loadLM(dir + "/bigram.sjlm")
        if code != 0 { log.error("shanjie_engine_load_lm failed, code \(code)"); return }
        e.setProfile(0)  // chat
        engine = e
        log.notice("engine loaded in \(Date().timeIntervalSince(t0), format: .fixed(precision: 2), privacy: .public) s")
        Footprint.record("after_load")
    }
}
