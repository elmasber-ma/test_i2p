import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../services/i2p_service.dart';

class I2pTestScreen extends StatefulWidget {
  const I2pTestScreen({super.key});

  @override
  State<I2pTestScreen> createState() => _I2pTestScreenState();
}

class _I2pTestScreenState extends State<I2pTestScreen> {
  final I2pService _s = I2pService.instance;
  List<String> _rustLogs = [];

  @override
  void initState() {
    super.initState();
    _s.refresh();
    _loadRustLogs();
  }

  Future<void> _loadRustLogs() async {
    final logs = await _s.getLogs();
    if (mounted) setState(() => _rustLogs = logs);
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        title: const Text('I2P · opciones del router'),
        actions: [
          if (_rustLogs.isNotEmpty)
            IconButton(
              icon: const Icon(Icons.copy, size: 20),
              tooltip: 'Copiar logs',
              onPressed: () {
                final text = _rustLogs.join('\n');
                Clipboard.setData(ClipboardData(text: text));
                ScaffoldMessenger.of(context).showSnackBar(
                  const SnackBar(
                      content: Text('Logs copiados'), duration: Duration(seconds: 1)),
                );
              },
            ),
        ],
      ),
      body: ListenableBuilder(
        listenable: _s,
        builder: (_, __) {
          return ListView(
            padding: const EdgeInsets.all(12),
            children: [
              Card(
                child: Padding(
                  padding: const EdgeInsets.all(12),
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Row(children: [
                        Icon(
                          Icons.hub,
                          color: _s.running
                              ? (_s.state.contains('listo')
                                  ? Colors.green
                                  : Colors.orange)
                              : Colors.grey,
                        ),
                        const SizedBox(width: 8),
                        Expanded(
                          child: Text(
                            _s.busy ? 'trabajando…' : _s.state,
                            style: const TextStyle(fontWeight: FontWeight.bold),
                          ),
                        ),
                      ]),
                      if (_s.samPort != null)
                        Padding(
                          padding: const EdgeInsets.only(top: 4),
                          child: SelectableText(
                            'SAMv3: 127.0.0.1:${_s.samPort} · directo',
                          ),
                        ),
                    ],
                  ),
                ),
              ),
              Row(children: [
                Expanded(
                  child: FilledButton.icon(
                    icon: Icon(_s.running
                        ? Icons.stop_rounded
                        : Icons.play_arrow_rounded),
                    label: Text(_s.running ? 'Detener' : 'Iniciar router'),
                    onPressed: _s.busy
                        ? null
                        : () async {
                            if (_s.running) {
                              await _s.stop();
                            } else {
                              await _s.start();
                            }
                            _loadRustLogs();
                          },
                  ),
                ),
                const SizedBox(width: 8),
                OutlinedButton(
                  onPressed: _s.running
                      ? () async {
                          await _s.probeSam();
                          _loadRustLogs();
                        }
                      : null,
                  child: const Text('Sonda SAM'),
                ),
              ]),
              SwitchListTile(
                contentPadding: EdgeInsets.zero,
                dense: true,
                title: const Text('Publicar mi dirección (inbound)',
                    style: TextStyle(fontSize: 13)),
                subtitle: const Text(
                  'Solo con IP real + puerto forwardeado; tras CGNAT '
                  'dejalo apagado',
                  style: TextStyle(fontSize: 11),
                ),
                value: _s.publicar,
                onChanged:
                    _s.running ? null : (v) => setState(() => _s.publicar = v),
              ),
              const Divider(height: 24),
              // ---------- logs Rust (reseed por URL)
              Row(
                children: [
                  const Expanded(
                    child: Text('Log reseed',
                        style: TextStyle(
                            fontSize: 13, fontWeight: FontWeight.bold)),
                  ),
                  TextButton(
                    onPressed: () async {
                      await _s.clearLogs();
                      _loadRustLogs();
                    },
                    child: const Text('Limpiar', style: TextStyle(fontSize: 11)),
                  ),
                ],
              ),
              if (_rustLogs.isEmpty)
                const Padding(
                  padding: EdgeInsets.all(8),
                  child: Text('Sin logs',
                      style: TextStyle(fontSize: 11, color: Colors.grey)),
                )
              else
                Container(
                  width: double.infinity,
                  constraints: const BoxConstraints(maxHeight: 400),
                  padding: const EdgeInsets.all(8),
                  decoration: BoxDecoration(
                    color: Colors.black87,
                    borderRadius: BorderRadius.circular(8),
                  ),
                  child: SingleChildScrollView(
                    child: SelectableText(
                      _rustLogs.join('\n'),
                      style: const TextStyle(
                        fontSize: 11,
                        fontFamily: 'monospace',
                        color: Colors.greenAccent,
                      ),
                    ),
                  ),
                ),
              const SizedBox(height: 12),
            ],
          );
        },
      ),
    );
  }
}
