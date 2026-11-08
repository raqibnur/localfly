import AppKit
import SwiftUI

@main
struct LocalflyApp: App {
    @NSApplicationDelegateAdaptor private var delegate: AppDelegate
    @StateObject private var model = AppModel()

    var body: some Scene {
        Window("Localfly", id: "main") {
            MainWindow()
                .environmentObject(model)
                .frame(minWidth: 760, minHeight: 500)
                .onAppear { model.applyActivationPolicy() }
        }
        .defaultSize(width: 900, height: 600)
        .windowToolbarStyle(.unified(showsTitle: false))
        .commands {
            CommandGroup(replacing: .newItem) {}
        }

        MenuBarExtra {
            MenuBarPanel()
                .environmentObject(model)
        } label: {
            Image(systemName: model.anyActive ? "paperplane.fill" : "paperplane")
                .accessibilityLabel(model.liveCount > 0 ? "Localfly, \(model.liveCount) live" : "Localfly")
        }
        .menuBarExtraStyle(.window)
    }
}

final class AppDelegate: NSObject, NSApplicationDelegate {
    /// Closing the window keeps tunnels running in the menu bar.
    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { false }
}
