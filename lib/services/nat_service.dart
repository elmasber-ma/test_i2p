import 'dart:async';

import 'package:port_forwarder/port_forwarder.dart';

/// Mapeo de puerto activo en el router (para poder cerrarlo al salir).
class NatMapping {
  final PortType protocol;
  final int externalPort;
  final int internalPort;
  final String description;
  NatMapping({
    required this.protocol,
    required this.externalPort,
    required this.internalPort,
    required this.description,
  });
}

/// Servicio global de UPnP / NAT-PMP / NAT-PCP. Envuelve `port_forwarder`
/// (Dart puro, sin Rust). Siempre activo: no depende de Settings.
///
/// Uso I2P:
///   await NatService.instance.openTcp(localPort: 25515);
///   await NatService.instance.openUdp(localPort: 25515);
class NatService {
  static final NatService instance = NatService._();
  NatService._();

  Gateway? _gateway;
  final List<NatMapping> _mappings = [];

  Gateway? get gateway => _gateway;

  List<NatMapping> get mappings => List.unmodifiable(_mappings);

  /// Descubre el gateway (UPnP / NAT-PMP / NAT-PCP). Idempotente.
  Future<Gateway?> discover() async {
    if (_gateway != null) return _gateway;
    try {
      _gateway = await Gateway.discover();
    } catch (e) {
      // ignore: avoid_print
      print('NatService.discover error: $e');
      _gateway = null;
    }
    return _gateway;
  }

  /// Abre [localPort] en el router. Devuelve el puerto externo mapeado,
  /// o null si no hay gateway / falló.
  Future<int?> openPort({
    required PortType protocol,
    required int localPort,
    int? externalPort,
    String description = 'test_i2p',
  }) async {
    final gw = await discover();
    if (gw == null) return null;
    final ext = externalPort ?? localPort;
    try {
      final ok = await gw.openPort(
        protocol: protocol,
        externalPort: ext,
        internalPort: localPort,
        portDescription: description,
      );
      if (ok) {
        _mappings.add(NatMapping(
          protocol: protocol,
          externalPort: ext,
          internalPort: localPort,
          description: description,
        ));
        return ext;
      }
      return null;
    } catch (e) {
      // ignore: avoid_print
      print('NatService.openPort error: $e');
      return null;
    }
  }

  /// Helper UDP (caso I2P transporte).
  Future<int?> openUdp({
    required int localPort,
    int? externalPort,
    String description = 'test_i2p',
  }) =>
      openPort(
        protocol: PortType.udp,
        localPort: localPort,
        externalPort: externalPort,
        description: description,
      );

  /// Helper TCP.
  Future<int?> openTcp({
    required int localPort,
    int? externalPort,
    String description = 'test_i2p',
  }) =>
      openPort(
        protocol: PortType.tcp,
        localPort: localPort,
        externalPort: externalPort,
        description: description,
      );

  Future<bool> closePort({
    required PortType protocol,
    required int externalPort,
  }) async {
    final gw = _gateway;
    if (gw == null) return false;
    var ok = false;
    try {
      ok = await gw.closePort(protocol: protocol, externalPort: externalPort);
    } catch (e) {
      // ignore: avoid_print
      print('NatService.closePort error: $e');
    }
    _mappings.removeWhere(
        (m) => m.protocol == protocol && m.externalPort == externalPort);
    return ok;
  }

  /// Cierra todos los mapeos abiertos (al detener I2P o salir).
  Future<void> closeAll() async {
    final gw = _gateway;
    if (gw == null) {
      _mappings.clear();
      return;
    }
    for (final m in List<NatMapping>.from(_mappings)) {
      try {
        await gw.closePort(protocol: m.protocol, externalPort: m.externalPort);
      } catch (_) {}
    }
    _mappings.clear();
  }

  Future<String?> externalIp() async {
    final gw = await discover();
    if (gw == null) return null;
    try {
      return (await gw.externalAddress).address;
    } catch (e) {
      // ignore: avoid_print
      print('NatService.externalIp error: $e');
      return null;
    }
  }
}
