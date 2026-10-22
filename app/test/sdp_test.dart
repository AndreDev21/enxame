import 'package:enxame_app/src/calls.dart';
import 'package:flutter_test/flutter_test.dart';

const _sdp =
    'v=0\r\n'
    'o=- 1 2 IN IP4 127.0.0.1\r\n'
    's=-\r\n'
    'm=audio 9 UDP/TLS/RTP/SAVPF 111\r\n'
    'c=IN IP4 0.0.0.0\r\n'
    'a=fmtp:111 minptime=10;useinbandfec=1\r\n'
    'm=video 9 UDP/TLS/RTP/SAVPF 96 97 98\r\n'
    'c=IN IP4 0.0.0.0\r\n'
    'a=rtpmap:96 VP8/90000\r\n'
    'a=rtpmap:97 rtx/90000\r\n'
    'a=fmtp:97 apt=96\r\n'
    'a=rtpmap:98 H264/90000\r\n'
    'a=fmtp:98 level-asymmetry-allowed=1;packetization-mode=1\r\n';

void main() {
  test('boostVideoBitrate só mexe na seção de vídeo', () {
    final out = boostVideoBitrate(_sdp);
    final lines = out.split('\r\n');
    // Áudio intacto.
    expect(lines, contains('a=fmtp:111 minptime=10;useinbandfec=1'));
    // Vídeo: limite de banda logo depois do c= e fmtp com os parâmetros.
    final v = lines.indexWhere((l) => l.startsWith('m=video'));
    expect(lines[v + 1], 'c=IN IP4 0.0.0.0');
    expect(lines[v + 2], 'b=AS:8000');
    expect(
      lines.where(
        (l) =>
            l.startsWith('a=fmtp:98') &&
            l.contains('x-google-start-bitrate=4000'),
      ),
      hasLength(1),
    );
    // rtx (apt=) não ganha parâmetros de bitrate; só há um b=AS e a quebra de linha é CRLF.
    expect(lines, contains('a=fmtp:97 apt=96'));
    expect(lines.where((l) => l.startsWith('b=AS:')), hasLength(1));
    expect(
      out.contains('\n') && !out.replaceAll('\r\n', '').contains('\n'),
      isTrue,
    );
  });

  test('é idempotente: aplicar de novo não duplica', () {
    final once = boostVideoBitrate(_sdp);
    final twice = boostVideoBitrate(once);
    expect(twice, once);
  });
}
