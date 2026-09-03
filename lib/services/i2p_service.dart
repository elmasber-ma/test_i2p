import 'dart:async';
import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:path_provider/path_provider.dart';
import 'package:permission_handler/permission_handler.dart';

import '../src/rust/api/i2p.dart' as rust;
import 'nat_service.dart';

/// I2P embebido vía emissary: singleton que gestiona el ciclo de vida del
/// router (habla DIRECTO por SAMv3, sin puente local) y expone GET y
/// descarga por eepsites.
///
/// Nota FRB: bindings async aunque el Rust sea sync → [refresh] cachea
/// running/port/fase para getters sync (mismo patrón que TorService).
class I2pService extends ChangeNotifier {
  I2pService._();
  static final I2pService instance = I2pService._();

  bool _busy = false;
  String _state = 'apagado';
  bool _running = false;
  int? _samPort;
  // Salir a red: publicar + puerto fijo + UPnP siempre activo.
  bool _publicar = true;
  final _log = <String>[];

  /// Puertos fijos para salir a red (wifi casa UPnP).
  /// SAM localhost 7656 clásico I2P, transporte NTCP2/SSU2 25515 oficial.
  static const int samFixed = 7656;
  static const int transportFixed = 25515;

  /// Reseed hosts que Dart pasa a Rust. Editables desde la UI o config.
  List<String> reseedHosts = const [
    'https://reseed.stormycloud.org/',
    'https://reseed-pl.i2pd.xyz/',
    'https://reseed-fr.i2pd.xyz/',
    'https://www2.mk16.de/',
    'https://reseed2.i2p.net/',
    'https://banana.incognet.io/',
    'https://reseed.diva.exchange/',
    'https://reseed.i2pgit.org/',
    'https://i2p.novg.net/',
    'https://reseed.onion.im/',
    'https://reseed.memcpy.io/',
    'https://i2pseed.creativecowpat.net:8443/',
    'https://reseed.sahil.world/',
    'https://i2p.diyarciftci.xyz/',
    'https://spiral.likogan.dev/',
  ];

  bool get busy => _busy;
  String get state => _state;
  bool get running => _running;
  int? get samPort => _samPort;
  String _netinfo = '';
  String get netinfo => _netinfo;

  /// Publicar direcciones de transporte en NetDb (salir a red).
  /// Requiere IP alcanzable + UPnP en wifi casa; tras CGNAT no sirve.
  /// Default on (siempre UPnP según orden).
  bool get publicar => _publicar;
  set publicar(bool v) {
    _publicar = v;
    notifyListeners();
  }

  List<String> get log => List.unmodifiable(_log);

  /// Obtener logs — ahora solo Dart (Rust ya incluye reseed en el mensaje de start)
  Future<List<String>> getLogs() async => List.unmodifiable(_log);

  Future<void> clearLogs() async {
    _log.clear();
    notifyListeners();
  }

  /// Guarda copia pública del reseed en Download (opcional, requiere permiso).
  /// Privado siempre se guarda en appSupport/i2p_data/.emissary vía Rust Storage.
  Future<String> saveReseedPublic() async {
    final support = await getApplicationSupportDirectory();
    final privateDir = Directory('${support.path}/i2p_data/.emissary');
    if (!privateDir.existsSync()) throw 'sin datos privados aún (inicia I2P primero)';
    // pedir permiso en Android 11+
    if (Platform.isAndroid) {
      final s = await Permission.manageExternalStorage.request();
      if (!s.isGranted) {
        final s2 = await Permission.storage.request();
        if (!s2.isGranted) throw 'permiso denegado';
      }
    }
    final downloadDir = Directory('/storage/emulated/0/Download');
    if (!downloadDir.existsSync()) throw 'Download no encontrado';
    final dest = Directory('${downloadDir.path}/i2p_reseed_${DateTime.now().millisecondsSinceEpoch}');
    await dest.create(recursive: true);
    int n = 0;
    for (final e in privateDir.listSync(recursive: true)) {
      if (e is File) {
        final rel = e.path.substring(privateDir.path.length + 1);
        final target = File('${dest.path}/$rel');
        await target.parent.create(recursive: true);
        await e.copy(target.path);
        n++;
      }
    }
    final msg = 'copiado $n archivos privado→público: ${dest.path}';
    _say(msg);
    return dest.path;
  }

  Future<String> getPrivatePath() async {
    final support = await getApplicationSupportDirectory();
    return '${support.path}/i2p_data/.emissary';
  }

  void _say(String m) {
    debugPrint('[i2p] $m');
    _log.insert(0, m);
    if (_log.length > 40) _log.removeLast();
    notifyListeners();
  }

  /// Estado humano reportado por Rust (apagado/bootstrapeando/corriendo/listo)
  /// + netinfo en vivo (conectados/túneles/tránsito).
  Future<void> refresh() async {
    try {
      _running = await rust.i2PIsRunning();
      _samPort = await rust.i2PSamPort();
      _state = await rust.i2PEstado();
      if (_running) _netinfo = await rust.i2PNetinfo();
    } catch (_) {}
    notifyListeners();
  }

  Future<int> _freePort() async {
    final s = await ServerSocket.bind(
        InternetAddress.anyIPv4, 0,
        v6Only: false);
    final p = s.port;
    await s.close();
    return p;
  }

  /// Intenta bindear [port] fijo; si está ocupado usa uno libre al azar.
  Future<int> _takePort(int port, String nombre) async {
    try {
      final s = await ServerSocket.bind(InternetAddress.anyIPv4, port,
          v6Only: false);
      await s.close();
      return port;
    } catch (_) {
      final p = await _freePort();
      _say('$nombre fijo $port ocupado, usando $p');
      return p;
    }
  }

  /// Arranca el router (persiste a nivel app hasta stop explícito).
  Future<void> start() async {
    if (_busy || _running) return;
    _busy = true;
    _state = 'bootstrapeando/reseeding…';
    notifyListeners();
    try {
      final support = await getApplicationSupportDirectory();
      final dir = Directory('${support.path}/i2p_data');
      if (!dir.existsSync()) dir.createSync(recursive: true);
      // app tiene m3u/su3 embebido (rust/assets/i2pseeds*.su3) — no mira Download (evita Permission denied)
      _say('usando su3 embebido + Reseeder si hace falta');

      final sam = await _takePort(samFixed, 'SAM');
      final trans = await _takePort(transportFixed, 'transporte');
      // UPnP siempre activo: mapear transporte TCP+UDP antes de arrancar.
      _say('UPnP: buscando gateway…');
      final gw = await NatService.instance.discover();
      if (gw == null) {
        _say('UPnP: sin gateway, sigo solo outbound');
      } else {
        final tcp = await NatService.instance
            .openTcp(localPort: trans, description: 'i2p-ntcp2');
        var udp = await NatService.instance
            .openUdp(localPort: trans, description: 'i2p-ssu2');
        // reintento UDP una vez: varios gateways lo rechazan al primer intento
        if (udp == null) {
          await Future.delayed(const Duration(seconds: 2));
          udp = await NatService.instance
              .openUdp(localPort: trans, description: 'i2p-ssu2');
        }
        final extIp = await NatService.instance.externalIp();
        if (tcp != null && udp != null) {
          _say('UPnP: gw ok${extIp != null ? ' ip=$extIp' : ''} '
              'tcp=$tcp udp=$udp (local $trans)');
        } else if (tcp != null) {
          _say('UPnP: gw ok${extIp != null ? ' ip=$extIp' : ''} '
              'tcp=$tcp ok (NTCP2 inbound) udp=fail (SSU2 solo outbound, normal en varios routers)');
        } else {
          _say('UPnP: mapeo falló${extIp != null ? ' ip=$extIp' : ''} '
              '(local $trans) — sigo solo outbound');
        }
      }
      _publicar = true;
      final msg = await rust.i2PStart(
          dataDir: dir.path,
          samPort: sam,
          transportPort: trans,
          publicar: _publicar,
          reseedHosts: reseedHosts);
      _state = 'corriendo';
      for (final line in msg.split('\n').where((l) => l.isNotEmpty)) {
        _say(line);
      }
    } catch (e) {
      _state = 'error';
      for (final line in e.toString().split('\n').where((l) => l.isNotEmpty)) {
        _say(line.startsWith('ERROR:') ? line : 'ERROR: $line');
      }
    } finally {
      _busy = false;
      await refresh();
    }
  }

  Future<void> stop() async {
    if (_busy) return;
    try {
      await rust.i2PStop();
      _state = 'apagado';
      _netinfo = '';
      _say('router detenido');
      await NatService.instance.closeAll();
      _say('UPnP: mapeos cerrados');
    } catch (e) {
      _say('ERROR stop: $e');
    }
    await refresh();
  }

  /// Sonda SAM (sube el estado a "listo" al primer OK).
  Future<String> probeSam() async {
    try {
      final r = await rust.i2PProbeSam();
      _say(r);
      await refresh();
      return r;
    } catch (e) {
      _say('ERROR probe: $e');
      return 'error: $e';
    }
  }

  /// GET directo por I2P sobre un destino .i2p / b32.
  Future<String> httpGet(String url) async {
    if (!_running) throw 'I2P no está corriendo';
    try {
      final r = await rust.i2PHttpGet(url: url);
      _say('GET $url ok (${r.length} B)');
      return r;
    } catch (e) {
      _say('GET $url falló: ${e.toString().split('\n').first}');
      rethrow;
    }
  }

  /// Descarga streaming a archivo POR I2P con progreso opcional.
  Future<String> download(
    String url, {
    required String savePath,
    void Function(int got)? onProgress,
  }) async {
    if (!_running) throw 'I2P no está corriendo';
    // El Rust descarga completo y devuelve bytes; para progreso vivo
    // consultamos el tamaño parcial del archivo mientras corre.
    Timer? ticker;
    try {
      ticker = Timer.periodic(const Duration(milliseconds: 400), (_) {
        final f = File(savePath);
        if (f.existsSync()) onProgress?.call(f.lengthSync());
      });
      final n =
          await rust.i2PDownload(url: url, destPath: savePath);
      _say('descargado $url → $savePath ($n B)');
      return savePath;
    } catch (e) {
      _say('descarga falló: ${e.toString().split('\n').first}');
      rethrow;
    } finally {
      ticker?.cancel();
    }
  }
}
