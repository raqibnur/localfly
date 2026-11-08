import LocalflyCore
import SwiftUI

/// Sheet for adding or editing a saved app.
struct AppEditor: View {
    @EnvironmentObject private var model: AppModel
    @Environment(\.dismiss) private var dismiss
    let isNew: Bool
    @State private var draft: SavedApp
    @State private var portText: String
    @State private var publicPortText: String
    @State private var addressText: String
    @State private var probe: Bool?
    @FocusState private var nameFocused: Bool

    init(draft: SavedApp, isNew: Bool) {
        self.isNew = isNew
        _draft = State(initialValue: draft)
        _portText = State(initialValue: String(draft.port))
        _publicPortText = State(initialValue: draft.publicPort.map(String.init) ?? "")
        _addressText = State(initialValue: draft.publicAddress ?? "")
    }

    private var port: UInt16? { UInt16(portText).flatMap { $0 == 0 ? nil : $0 } }
    private var name: String { draft.name.trimmingCharacters(in: .whitespaces) }
    private var duplicate: SavedApp? { model.apps.first { $0.port == port && $0.id != draft.id } }
    /// Empty means random; anything else must be a valid TCP port.
    private var publicPort: UInt16?? {
        if publicPortText.isEmpty { return .some(nil) }
        return UInt16(publicPortText).flatMap { $0 == 0 ? nil : .some($0) }
    }
    /// Empty means none; a bare host gets https://.
    private var address: String?? {
        let trimmed = addressText.trimmingCharacters(in: .whitespacesAndNewlines)
        if trimmed.isEmpty { return .some(nil) }
        let full = trimmed.contains("://") ? trimmed : "https://\(trimmed)"
        guard let url = URL(string: full), let scheme = url.scheme, ["http", "https"].contains(scheme), url.host?.isEmpty == false else { return nil }
        return .some(full)
    }
    private var publicPortClash: SavedApp? {
        guard case let port?? = publicPort else { return nil }
        return model.apps.first { $0.publicPort == port && $0.id != draft.id }
    }
    private var canSave: Bool { !name.isEmpty && port != nil && publicPort != nil && address != nil }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: 12) {
                Monogram(app: preview, size: 40)
                    .animation(.easeOut(duration: 0.2), value: preview.hue)
                VStack(alignment: .leading, spacing: 2) {
                    Text(isNew ? "Add App" : "Edit App").font(.system(size: 16, weight: .semibold))
                    Text("Anything served on localhost: web apps, APIs, webhooks.")
                        .font(.system(size: 12)).foregroundStyle(.secondary)
                }
            }
            .padding(20)

            Form {
                TextField("Name", text: $draft.name, prompt: Text("e.g. web"))
                    .focused($nameFocused)
                TextField("Port", text: $portText, prompt: Text("3000"))
                    .font(.system(.body, design: .monospaced))
                    .onChange(of: portText) { text in
                        let digits = String(text.filter(\.isNumber).prefix(5))
                        if digits != text { portText = digits }
                    }
                Toggle("Start tunnel when Localfly opens", isOn: $draft.startOnLaunch)

                Section {
                    TextField("Public port", text: $publicPortText, prompt: Text("Random"))
                        .font(.system(.body, design: .monospaced))
                        .onChange(of: publicPortText) { text in
                            let digits = String(text.filter(\.isNumber).prefix(5))
                            if digits != text { publicPortText = digits }
                        }
                    TextField("Address", text: $addressText, prompt: Text("https://web.yourdomain.com"))
                        .font(.system(.body, design: .monospaced))
                } header: {
                    Text("Your relay")
                } footer: {
                    publicStatus
                }
            }
            .formStyle(.grouped)
            .scrollDisabled(true)
            .frame(height: 318)

            portStatus
                .padding(.horizontal, 20)
                .frame(height: 22, alignment: .leading)

            HStack {
                Spacer()
                Button("Cancel", role: .cancel) { dismiss() }
                    .keyboardShortcut(.cancelAction)
                Button(isNew ? "Add App" : "Save") { save() }
                    .keyboardShortcut(.defaultAction)
                    .buttonStyle(.borderedProminent)
                    .tint(.flare)
                    .disabled(!canSave)
            }
            .padding(20)
        }
        .frame(width: 420)
        .onAppear { nameFocused = true }
        .task(id: port) {
            probe = nil
            guard let port else { return }
            try? await Task.sleep(nanoseconds: 250_000_000)
            probe = await PortProbe.isListening(port)
        }
    }

    private var preview: SavedApp {
        var copy = draft
        copy.hue = SavedApp.hue(for: name.isEmpty ? "?" : name)
        if name.isEmpty { copy.name = "?" }
        return copy
    }

    @ViewBuilder private var publicStatus: some View {
        Group {
            if publicPort == nil {
                Label("Public port must be 1–65535 (bore servers allow 1024 and up by default)", systemImage: "exclamationmark.circle")
                    .foregroundStyle(Color.pending)
            } else if let clash = publicPortClash {
                Label("\(clash.name) already asks for this public port", systemImage: "exclamationmark.circle")
                    .foregroundStyle(Color.pending)
            } else if address == nil {
                Label("Enter an http(s) address, like https://web.yourdomain.com", systemImage: "exclamationmark.circle")
                    .foregroundStyle(Color.pending)
            } else if case .some(.some) = address, case .some(.none) = publicPort {
                Label("Set a public port too, so your proxy knows where to send this address", systemImage: "info.circle")
                    .foregroundStyle(.secondary)
            } else {
                Text("For your own bore server. Point the address at the public port in Caddy for HTTPS. Leave both empty on bore.pub.")
                    .foregroundStyle(.secondary)
            }
        }
        .font(.system(size: 11.5))
        .fixedSize(horizontal: false, vertical: true)
    }

    @ViewBuilder private var portStatus: some View {
        Group {
            if let duplicate {
                Label("\(duplicate.name) already uses :\(String(port ?? 0))", systemImage: "exclamationmark.circle")
                    .foregroundStyle(Color.pending)
            } else if port == nil {
                Label("Enter a port between 1 and 65535", systemImage: "number")
                    .foregroundStyle(.secondary)
            } else if probe == true {
                HStack(spacing: 6) { StatusDot(color: .live, pulsing: true, size: 6); Text("Something is running on :\(String(port!))") }
                    .foregroundStyle(Color.live)
            } else if probe == false {
                Label("Nothing on :\(String(port!)) yet. You can still save it.", systemImage: "moon.zzz")
                    .foregroundStyle(.secondary)
            }
        }
        .font(.system(size: 12, weight: .medium))
        .transition(.opacity)
    }

    private func save() {
        guard let port, canSave, case let publicPort? = publicPort, case let address? = address else { return }
        draft.name = name
        draft.port = port
        draft.publicPort = publicPort
        draft.publicAddress = address
        if isNew {
            draft.hue = SavedApp.hue(for: name)
            model.add(draft)
        } else {
            model.update(draft)
        }
        dismiss()
    }
}
