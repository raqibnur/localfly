import LocalflyCore
import SwiftUI

struct PageHeader: View {
    let title: String
    let subtitle: String

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(title).font(.system(size: 30, weight: .bold))
            Text(subtitle).font(.system(size: 13)).foregroundStyle(.secondary)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(.horizontal, 28)
        .padding(.top, 8)
    }
}

struct RelayView: View {
    @EnvironmentObject private var model: AppModel
    @State private var relayDraft = ""
    @State private var checking = false

    var body: some View {
        VStack(spacing: 0) {
            PageHeader(title: "Relay", subtitle: "The server visitors reach. Your Mac dials out to it, so no ports open on your network.")
            Form {
                Section {
                    LabeledContent("Server") {
                        TextField("Server", text: $relayDraft, prompt: Text("bore.pub"))
                            .labelsHidden()
                            .multilineTextAlignment(.trailing)
                            .font(.system(.body, design: .monospaced))
                            .onSubmit(commit)
                    }
                    LabeledContent("Status") {
                        HStack(spacing: 8) {
                            if checking {
                                ProgressView().controlSize(.small).scaleEffect(0.7).frame(width: 14, height: 14)
                            } else {
                                SignalBars(latency: model.relayLatency, reachable: model.relayReachable)
                            }
                            Text(statusText).foregroundStyle(model.relayReachable == false ? Color.fault : .secondary)
                            Button("Check") { check() }.controlSize(.small)
                        }
                    }
                } footer: {
                    Text("Use host or host:port. bore.pub is free and public; addresses change each time a tunnel opens. Point this at your own bore server for private, steadier addresses.")
                }

                Section {
                    LabeledContent("Secret") {
                        SecureField("Secret", text: $model.secret, prompt: Text("None"))
                            .labelsHidden()
                            .multilineTextAlignment(.trailing)
                    }
                } footer: {
                    Text("Only for relays started with --secret. Stored in your keychain.")
                }

                Section {
                    Toggle(isOn: $model.autoReconnect) {
                        Text("Reconnect automatically")
                        Text("Retries with backoff after Wi-Fi drops or sleep, and asks for the same public port first.")
                    }
                }

                if relayDraft != model.relay {
                    HStack {
                        Spacer()
                        Button("Revert") { relayDraft = model.relay }
                        Button("Use \(relayDraft.isEmpty ? "bore.pub" : relayDraft)", action: commit)
                            .buttonStyle(.borderedProminent).tint(.flare)
                    }
                    .listRowBackground(Color.clear)
                }
            }
            .formStyle(.grouped)
        }
        .onAppear { relayDraft = model.relay }
    }

    private var statusText: String {
        switch model.relayReachable {
        case true?: "Reachable · \(Format.latency(model.relayLatency))"
        case false?: "Unreachable"
        case nil: "Checking…"
        }
    }

    private func commit() {
        let value = relayDraft.trimmingCharacters(in: .whitespaces)
        model.relay = value.isEmpty ? "bore.pub" : value
        relayDraft = model.relay
    }

    private func check() {
        checking = true
        Task {
            await model.refreshRelay()
            checking = false
        }
    }
}

struct GeneralView: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        VStack(spacing: 0) {
            PageHeader(title: "General", subtitle: "How Localfly behaves on your Mac.")
            Form {
                Section {
                    Toggle(isOn: Binding(get: { model.launchAtLogin }, set: { model.setLaunchAtLogin($0) })) {
                        Text("Open at login")
                        Text("Starts quietly in the menu bar. Apps marked “Start on launch” come online by themselves.")
                    }
                    if let error = model.launchAtLoginError {
                        Label(error, systemImage: "exclamationmark.triangle").foregroundStyle(Color.fault).font(.caption)
                    }
                    Toggle(isOn: $model.showDockIcon) {
                        Text("Show in Dock")
                        Text("Localfly always lives in the menu bar.")
                    }
                }
                Section {
                    Toggle(isOn: $model.checkForUpdates) {
                        Text("Check for updates")
                        Text("Asks GitHub once a day whether a newer release exists. Nothing else is sent.")
                    }
                }
                Section {
                    LabeledContent("Running tunnels") {
                        Button("Stop All") { model.stopAll() }.disabled(!model.anyActive)
                    }
                }
            }
            .formStyle(.grouped)
        }
    }
}

struct AboutView: View {
    private var version: String {
        let info = Bundle.main.infoDictionary
        return "\(info?["CFBundleShortVersionString"] as? String ?? "0.1.0") (\(info?["CFBundleVersion"] as? String ?? "1"))"
    }

    var body: some View {
        ScrollView {
            VStack(spacing: 22) {
                VStack(spacing: 12) {
                    BrandMark(size: 72)
                    Text("Localfly").font(.system(size: 22, weight: .bold))
                    Text("Version \(version)").font(.system(size: 12)).foregroundStyle(.secondary)
                    Text("Share what’s on localhost with anyone, anywhere.")
                        .foregroundStyle(.secondary)
                    HStack(spacing: 8) {
                        Link(destination: Links.repository) { Label("Source on GitHub", systemImage: "chevron.left.forwardslash.chevron.right") }
                        Text("·").foregroundStyle(.tertiary)
                        Link(destination: Links.issues) { Label("Report an issue", systemImage: "ladybug") }
                    }
                    .font(.system(size: 12, weight: .medium))
                    .padding(.top, 2)
                }
                .padding(.top, 36)

                SupportCard()
                    .frame(maxWidth: 520)

                VStack(alignment: .leading, spacing: 10) {
                    Text("ACKNOWLEDGEMENTS")
                        .font(.system(size: 10.5, weight: .semibold)).tracking(0.6).foregroundStyle(.secondary)
                    Text("Localfly speaks the bore tunnel protocol and works with any bore server, including the public bore.pub relay.")
                        .font(.system(size: 12.5))
                    DisclosureGroup("bore · MIT License") {
                        Text(Licenses.bore)
                            .font(.system(size: 11, design: .monospaced))
                            .foregroundStyle(.secondary)
                            .textSelection(.enabled)
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .padding(.top, 6)
                    }
                    .font(.system(size: 12.5, weight: .medium))
                }
                .padding(18)
                .surface()
                .frame(maxWidth: 520)
            }
            .frame(maxWidth: .infinity)
            .padding(.horizontal, 28)
            .padding(.bottom, 28)
        }
    }
}

enum Licenses {
    static let bore = """
    MIT License

    Copyright (c) 2022 Eric Zhang

    Permission is hereby granted, free of charge, to any person obtaining a copy \
    of this software and associated documentation files (the "Software"), to deal \
    in the Software without restriction, including without limitation the rights \
    to use, copy, modify, merge, publish, distribute, sublicense, and/or sell \
    copies of the Software, and to permit persons to whom the Software is \
    furnished to do so, subject to the following conditions:

    The above copyright notice and this permission notice shall be included in all \
    copies or substantial portions of the Software.

    THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR \
    IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, \
    FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE \
    AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER \
    LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, \
    OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE \
    SOFTWARE.
    """
}

/// Optional tip jar. Free and open source stays the default.
struct SupportCard: View {
    var body: some View {
        HStack(spacing: 14) {
            Image(systemName: "cup.and.saucer.fill")
                .font(.system(size: 16))
                .foregroundStyle(Color(red: 0.80, green: 0.52, blue: 0.25))
                .frame(width: 40, height: 40)
                .background(Circle().fill(Color(red: 0.80, green: 0.52, blue: 0.25).opacity(0.16)))
            VStack(alignment: .leading, spacing: 3) {
                Text("Free and open source").font(.system(size: 13, weight: .semibold))
                Text("If Localfly saves you time, a coffee helps keep it maintained.")
                    .font(.system(size: 12)).foregroundStyle(.secondary)
            }
            Spacer(minLength: 8)
            Link(destination: Links.coffee) { Text("Buy me a coffee") }
                .buttonStyle(.bordered)
        }
        .padding(16)
        .surface()
    }
}

/// Gauges interest in a hosted Pro version. Nothing here is sold yet.
struct ProView: View {
    private let ideas: [(symbol: String, title: String, detail: String)] = [
        ("lock.fill", "Instant HTTPS addresses", "Get yourapp.localfly.dev with a padlock in one click. No server, DNS or Caddy."),
        ("pin.fill", "Names that stay put", "The same address every time, so webhooks and shared links keep working."),
        ("globe", "Your own domain", "Serve from dev.yourcompany.com with certificates handled for you."),
        ("waveform.path.ecg", "Request inspector", "See every request and response, and replay webhooks while you debug."),
        ("key.fill", "Protected links", "Put a password or an email check in front of anything you share."),
    ]

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 22) {
                VStack(alignment: .leading, spacing: 8) {
                    HStack(spacing: 8) {
                        Text("Localfly Pro").font(.system(size: 30, weight: .bold))
                        Pill(text: "Idea stage", tint: .pending)
                    }
                    Text("Localfly is free and stays free, with your own relay or bore.pub. A hosted Pro version would remove the server setup. It only gets built if enough people want it.")
                        .font(.system(size: 13))
                        .foregroundStyle(.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                }

                VStack(spacing: 0) {
                    ForEach(Array(ideas.enumerated()), id: \.offset) { index, idea in
                        HStack(alignment: .top, spacing: 14) {
                            Image(systemName: idea.symbol)
                                .font(.system(size: 12, weight: .semibold))
                                .foregroundStyle(Color.flare)
                                .frame(width: 28, height: 28)
                                .background(RoundedRectangle(cornerRadius: 8, style: .continuous).fill(Color.flare.opacity(0.12)))
                            VStack(alignment: .leading, spacing: 2) {
                                Text(idea.title).font(.system(size: 13, weight: .semibold))
                                Text(idea.detail).font(.system(size: 12)).foregroundStyle(.secondary)
                                    .fixedSize(horizontal: false, vertical: true)
                            }
                            Spacer(minLength: 0)
                        }
                        .padding(.horizontal, 16)
                        .padding(.vertical, 12)
                        if index < ideas.count - 1 {
                            Rectangle().fill(Color.primary.opacity(0.06)).frame(height: 1).padding(.leading, 58)
                        }
                    }
                }
                .surface()

                HStack(spacing: 12) {
                    Link(destination: Links.proWaitlist) {
                        Label("I’m interested", systemImage: "hand.raised.fill")
                            .padding(.horizontal, 6)
                    }
                    .buttonStyle(.borderedProminent)
                    .tint(.flare)
                    .controlSize(.large)
                    Text("Opens a short sign-up form on the Localfly website.")
                        .font(.system(size: 12))
                        .foregroundStyle(.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
            .padding(.horizontal, 28)
            .padding(.top, 8)
            .padding(.bottom, 28)
            .frame(maxWidth: 680, alignment: .leading)
            .frame(maxWidth: .infinity)
        }
    }
}
