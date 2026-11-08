import LocalflyCore
import SwiftUI

enum Page: String, Hashable, CaseIterable {
    case apps, relay, general, pro, about

    var title: String {
        switch self {
        case .apps: "Apps"
        case .relay: "Relay"
        case .general: "General"
        case .pro: "Localfly Pro"
        case .about: "About"
        }
    }

    var symbol: String {
        switch self {
        case .apps: "square.stack.3d.up.fill"
        case .relay: "antenna.radiowaves.left.and.right"
        case .general: "gearshape.fill"
        case .pro: "sparkles"
        case .about: "info.circle.fill"
        }
    }

    var tint: Color {
        switch self {
        case .apps: .flare
        case .relay: Color(red: 0.36, green: 0.45, blue: 0.98)
        case .general: .gray
        case .pro: Color(red: 0.95, green: 0.55, blue: 0.20)
        case .about: Color(red: 0.55, green: 0.42, blue: 0.95)
        }
    }
}

struct MainWindow: View {
    @EnvironmentObject private var model: AppModel
    @AppStorage("page") private var section: Page = .apps

    var body: some View {
        NavigationSplitView {
            Sidebar(selection: $section)
                .navigationSplitViewColumnWidth(min: 200, ideal: 216, max: 260)
        } detail: {
            Group {
                switch section {
                case .apps: AppsView()
                case .relay: RelayView()
                case .general: GeneralView()
                case .pro: ProView()
                case .about: AboutView()
                }
            }
            .frame(minWidth: 520, maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
        }
        .tint(.flare)
        .onReceive(model.$wantsAddApp) { if $0 { section = .apps } }
    }
}

struct Sidebar: View {
    @EnvironmentObject private var model: AppModel
    @Binding var selection: Page

    var body: some View {
        List(selection: Binding(get: { selection }, set: { if let value = $0 { selection = value } })) {
            Section {
                row(.apps, badge: model.liveCount)
            }
            Section("Settings") {
                row(.relay)
                row(.general)
            }
            Section {
                row(.pro)
                row(.about)
            }
        }
        .listStyle(.sidebar)
        .safeAreaInset(edge: .top, spacing: 0) {
            HStack(spacing: 10) {
                BrandMark(size: 30)
                VStack(alignment: .leading, spacing: 1) {
                    Text("Localfly").font(.system(size: 14, weight: .bold))
                    Text("localhost, everywhere").font(.system(size: 11)).foregroundStyle(.secondary)
                }
                Spacer()
            }
            .padding(.horizontal, 16)
            .padding(.top, 4)
            .padding(.bottom, 10)
        }
        .safeAreaInset(edge: .bottom, spacing: 0) {
            VStack(spacing: 8) {
                if let update = model.availableUpdate {
                    UpdateCard(update: update)
                        .transition(.move(edge: .bottom).combined(with: .opacity))
                }
                AirStatus()
            }
            .padding(10)
        }
    }

    private func row(_ section: Page, badge: Int = 0) -> some View {
        Label {
            Text(section.title)
        } icon: {
            Image(systemName: section.symbol)
                .font(.system(size: 10.5, weight: .semibold))
                .foregroundStyle(.white)
                .frame(width: 20, height: 20)
                .background(RoundedRectangle(cornerRadius: 5.5, style: .continuous).fill(section.tint.gradient))
        }
        .badge(badge)
        .tag(section)
    }
}

/// Footer card in the sidebar summarising what is exposed right now.
struct AirStatus: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        HStack(spacing: 10) {
            StatusDot(color: model.liveCount > 0 ? .live : .secondary.opacity(0.6), pulsing: model.liveCount > 0, size: 8)
                .frame(width: 14)
            VStack(alignment: .leading, spacing: 1) {
                Text(model.liveCount == 0 ? "Nothing exposed" : model.liveCount == 1 ? "1 tunnel live" : "\(model.liveCount) tunnels live")
                    .font(.system(size: 12, weight: .semibold))
                Text(model.liveCount == 0 ? "Your apps are private" : "\(Format.bytes(model.totalTraffic)) moved")
                    .font(.system(size: 11).monospacedDigit())
                    .foregroundStyle(.secondary)
            }
            Spacer(minLength: 0)
            if model.anyActive {
                Button { model.stopAll() } label: {
                    Image(systemName: "stop.fill").font(.system(size: 9, weight: .bold)).frame(width: 22, height: 22)
                }
                .buttonStyle(PressableStyle())
                .foregroundStyle(.secondary)
                .help("Stop all tunnels")
            }
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 10)
        .surface(radius: 11)
        .animation(.easeOut(duration: 0.2), value: model.liveCount)
    }
}

/// Shown above the status card when GitHub has a newer release.
struct UpdateCard: View {
    let update: UpdateChecker.Release

    var body: some View {
        Button { NSWorkspace.shared.open(update.page) } label: {
            HStack(spacing: 10) {
                Image(systemName: "arrow.down.circle.fill")
                    .font(.system(size: 16))
                    .foregroundStyle(Color.flare)
                VStack(alignment: .leading, spacing: 1) {
                    Text("Version \(update.version) is out").font(.system(size: 12, weight: .semibold))
                    Text("Download from GitHub").font(.system(size: 11)).foregroundStyle(.secondary)
                }
                Spacer(minLength: 0)
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 9)
            .surface(radius: 11)
            .contentShape(Rectangle())
        }
        .buttonStyle(PressableStyle(hoverFill: false))
        .help("Open the release page")
    }
}
