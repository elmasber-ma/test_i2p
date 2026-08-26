import 'package:flutter/material.dart';

import 'screens/i2p_prueba_screen.dart';
import 'screens/i2p_test_screen.dart';

/// App mínima para probar I2P embebido (emissary) sin arrastrar el resto
/// del motor: compila rápido y aísla los errores de la red I2P.
///
/// Dos pestañas:
///  · EJEMPLOS → demo fija (stats.i2p) + campos libres de GET y descarga.
///  · TEST     → opciones del router: estado, iniciar/detener, publicar,
///               sonda SAM y registro en vivo.
void main() {
  runApp(const TestI2pApp());
}

class TestI2pApp extends StatelessWidget {
  const TestI2pApp({super.key});

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'test_i2p',
      theme: ThemeData(
        colorScheme: ColorScheme.fromSeed(seedColor: Colors.deepPurple),
        useMaterial3: true,
      ),
      home: const HomeTabs(),
    );
  }
}

class HomeTabs extends StatefulWidget {
  const HomeTabs({super.key});

  @override
  State<HomeTabs> createState() => _HomeTabsState();
}

class _HomeTabsState extends State<HomeTabs> {
  int _tab = 0;
  final _paginas = const [I2pPruebaScreen(), I2pTestScreen()];

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        title: Text(_tab == 0 ? 'I2P · EJEMPLOS' : 'I2P · TEST'),
      ),
      body: IndexedStack(index: _tab, children: _paginas),
      bottomNavigationBar: NavigationBar(
        selectedIndex: _tab,
        onDestinationSelected: (i) => setState(() => _tab = i),
        destinations: const [
          NavigationDestination(icon: Icon(Icons.public), label: 'EJEMPLOS'),
          NavigationDestination(icon: Icon(Icons.router), label: 'TEST'),
        ],
      ),
    );
  }
}
