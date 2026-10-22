import 'package:flutter/material.dart';
import 'package:flutter_webrtc/flutter_webrtc.dart';

import '../../l10n/app_localizations.dart';
import '../calls.dart';

/// Escolha da tela ou janela a transmitir (com miniaturas).
Future<DesktopCapturerSource?> showScreenPicker(BuildContext context) async {
  final l = L.of(context);
  final sources = await desktopCapturer.getSources(
    types: [SourceType.Screen, SourceType.Window],
    thumbnailSize: ThumbnailSize(320, 180),
  );
  if (!context.mounted) return null;
  return showDialog<DesktopCapturerSource>(
    context: context,
    builder: (c) => AlertDialog(
      title: Text(l.pickScreenTitle),
      content: SizedBox(
        width: 720,
        height: 420,
        child: sources.isEmpty
            ? Center(child: Text(l.pickScreenNone))
            : GridView.count(
                crossAxisCount: 3,
                childAspectRatio: 1.35,
                children: [
                  for (final s in sources)
                    InkWell(
                      onTap: () => Navigator.pop(c, s),
                      child: Card(
                        child: Column(
                          children: [
                            Expanded(
                              child: s.thumbnail == null
                                  ? const Icon(
                                      Icons.desktop_windows_outlined,
                                      size: 48,
                                    )
                                  : Image.memory(
                                      s.thumbnail!,
                                      fit: BoxFit.contain,
                                    ),
                            ),
                            Padding(
                              padding: const EdgeInsets.all(6),
                              child: Text(
                                '${s.type == SourceType.Screen ? l.pickScreenScreens : l.pickScreenWindows}: ${s.name}',
                                maxLines: 1,
                                overflow: TextOverflow.ellipsis,
                              ),
                            ),
                          ],
                        ),
                      ),
                    ),
                ],
              ),
      ),
      actions: [
        TextButton(onPressed: () => Navigator.pop(c), child: Text(l.cancel)),
      ],
    ),
  );
}

/// Camada sobre a janela: chamada entrando, chamando e em andamento.
class CallOverlay extends StatelessWidget {
  const CallOverlay({super.key, required this.controller});
  final CallController controller;

  @override
  Widget build(BuildContext context) {
    return ListenableBuilder(
      listenable: controller,
      builder: (context, _) {
        final l = L.of(context);
        final c = controller;
        if (c.phase == CallPhase.idle) return const SizedBox.shrink();
        final name = c.peerName ?? '';
        final t = Theme.of(context);
        final connected = c.phase == CallPhase.connected;
        return Material(
          color: t.colorScheme.surface.withValues(alpha: 0.97),
          child: SafeArea(
            child: Column(
              children: [
                if (c.sharing)
                  Container(
                    width: double.infinity,
                    color: t.colorScheme.error,
                    padding: const EdgeInsets.symmetric(
                      horizontal: 16,
                      vertical: 8,
                    ),
                    child: Row(
                      children: [
                        Icon(Icons.screen_share, color: t.colorScheme.onError),
                        const SizedBox(width: 8),
                        Expanded(
                          child: Text(
                            l.callSharing,
                            style: TextStyle(
                              color: t.colorScheme.onError,
                              fontWeight: FontWeight.w600,
                            ),
                          ),
                        ),
                        TextButton(
                          style: TextButton.styleFrom(
                            foregroundColor: t.colorScheme.onError,
                          ),
                          onPressed: c.stopShare,
                          child: Text(l.callStopShare),
                        ),
                      ],
                    ),
                  ),
                Expanded(
                  child: Stack(
                    children: [
                      Positioned.fill(
                        child: connected && c.remoteHasVideo
                            ? RTCVideoView(
                                c.remoteRenderer,
                                objectFit: RTCVideoViewObjectFit
                                    .RTCVideoViewObjectFitContain,
                              )
                            : Center(
                                child: Column(
                                  mainAxisSize: MainAxisSize.min,
                                  children: [
                                    CircleAvatar(
                                      radius: 44,
                                      child: Text(
                                        name.isEmpty
                                            ? '?'
                                            : name[0].toUpperCase(),
                                        style: t.textTheme.headlineLarge,
                                      ),
                                    ),
                                    const SizedBox(height: 16),
                                    Text(
                                      name,
                                      style: t.textTheme.headlineSmall,
                                    ),
                                    const SizedBox(height: 8),
                                    Text(switch (c.phase) {
                                      CallPhase.outgoing => l.callCalling(name),
                                      CallPhase.incoming => switch (c.kind) {
                                        CallKind.video => l.callIncomingVideo(
                                          name,
                                        ),
                                        CallKind.screen => l.callIncomingScreen(
                                          name,
                                        ),
                                        _ => l.callIncomingAudio(name),
                                      },
                                      CallPhase.connecting => l.callConnecting,
                                      _ =>
                                        c.kind == CallKind.screen
                                            ? l.callWaitingVideo
                                            : '',
                                    }),
                                  ],
                                ),
                              ),
                      ),
                      if (connected && (c.kind == CallKind.video || c.sharing))
                        Positioned(
                          right: 16,
                          bottom: 16,
                          width: 200,
                          height: 120,
                          child: ClipRRect(
                            borderRadius: BorderRadius.circular(8),
                            child: RTCVideoView(
                              c.localRenderer,
                              mirror: !c.sharing,
                            ),
                          ),
                        ),
                    ],
                  ),
                ),
                Padding(
                  padding: const EdgeInsets.all(16),
                  child: Row(
                    mainAxisAlignment: MainAxisAlignment.center,
                    children: [
                      if (c.phase == CallPhase.incoming) ...[
                        FilledButton.icon(
                          onPressed: c.accept,
                          icon: const Icon(Icons.call),
                          label: Text(l.accept),
                        ),
                        const SizedBox(width: 12),
                        FilledButton.icon(
                          style: FilledButton.styleFrom(
                            backgroundColor: t.colorScheme.error,
                          ),
                          onPressed: c.reject,
                          icon: const Icon(Icons.call_end),
                          label: Text(l.reject),
                        ),
                      ] else ...[
                        if (connected) ...[
                          IconButton.filledTonal(
                            tooltip: c.micOn ? l.callMute : l.callUnmute,
                            onPressed: c.toggleMic,
                            icon: Icon(c.micOn ? Icons.mic : Icons.mic_off),
                          ),
                          const SizedBox(width: 8),
                          if (c.kind == CallKind.video) ...[
                            IconButton.filledTonal(
                              tooltip: c.camOn
                                  ? l.callCameraOff
                                  : l.callCameraOn,
                              onPressed: c.toggleCamera,
                              icon: Icon(
                                c.camOn ? Icons.videocam : Icons.videocam_off,
                              ),
                            ),
                            const SizedBox(width: 8),
                          ],
                          IconButton.filledTonal(
                            tooltip: c.sharing
                                ? l.callStopShare
                                : l.callShareScreen,
                            onPressed: c.sharing ? c.stopShare : c.startShare,
                            icon: Icon(
                              c.sharing
                                  ? Icons.stop_screen_share
                                  : Icons.screen_share,
                            ),
                          ),
                          const SizedBox(width: 8),
                        ],
                        FilledButton.icon(
                          style: FilledButton.styleFrom(
                            backgroundColor: t.colorScheme.error,
                          ),
                          onPressed: c.hangup,
                          icon: const Icon(Icons.call_end),
                          label: Text(
                            c.phase == CallPhase.outgoing
                                ? l.callCancel
                                : l.callHangup,
                          ),
                        ),
                      ],
                    ],
                  ),
                ),
              ],
            ),
          ),
        );
      },
    );
  }
}
