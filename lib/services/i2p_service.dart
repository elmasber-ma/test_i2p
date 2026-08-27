import 'dart:async';
import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:path_provider/path_provider.dart';

import '../src/rust/api/i2p.dart' as rust;

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
  bool _publicar = false;
  final _log = <String>[];

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

  /// Publicar direcciones de transporte en NetDb (requiere IP real y
  /// puerto alcanzable; tras CGNAT no sirve). Default off.
  bool get publicar => _publicar;
  set publicar(bool v) {
    _publicar = v;
    notifyListeners();
  }

  List<String> get log => List.unmodifiable(_log);

  /// Obtener logs del Rust (reseed por URL con tiempos).
  Future<List<String>> getLogs() async {
    try {
      return await rust.i2pGetLogs();
    } catch (_) {
      return [];
    }
  }

  Future<void> clearLogs() async {
    try {
      await rust.i2pClearLogs();
    } catch (_) {}
    _log.clear();
    notifyListeners();
  }

  void _say(String m) {
    debugPrint('[i2p] $m');
    _log.insert(0, m);
    if (_log.length > 40) _log.removeLast();
    notifyListeners();
  }

  /// Estado humano reportado por Rust (apagado/bootstrapeando/corriendo/listo).
  Future<void> refresh() async {
    try {
      _running = await rust.i2PIsRunning();
      _samPort = await rust.i2PSamPort();
      _state = await rust.i2PEstado();
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

      final sam = await _freePort();
      final trans = await _freePort();
      final msg = await rust.i2PStart(
          dataDir: dir.path,
          samPort: sam,
          transportPort: trans,
          publicar: _publicar,
          reseedHosts: reseedHosts);
      _state = 'corriendo';
      _say(msg);
    } catch (e) {
      _state = 'error';
      _say('ERROR: $e');
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
      _say('router detenido');
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
