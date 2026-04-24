import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:vpn21/widgets/country_picker.dart';

Widget _host() => const ProviderScope(
      child: MaterialApp(
        home: Scaffold(body: Padding(
          padding: EdgeInsets.all(12),
          child: CountryPicker(),
        )),
      ),
    );

void main() {
  testWidgets('defaults to "Any" and allows selecting a country',
      (tester) async {
    await tester.pumpWidget(_host());
    await tester.pumpAndSettle();

    // Initial selection shows the "Any" label.
    expect(find.text('Exit: Any'), findsOneWidget);

    // Open the dropdown and pick Germany.
    await tester.tap(find.byType(DropdownButton<String>));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Exit: Germany').last);
    await tester.pumpAndSettle();

    expect(find.text('Exit: Germany'), findsOneWidget);
  });

  testWidgets('selectedCountryProvider updates when choice changes',
      (tester) async {
    final container = ProviderContainer();
    addTearDown(container.dispose);

    await tester.pumpWidget(UncontrolledProviderScope(
      container: container,
      child: const MaterialApp(
        home: Scaffold(
          body: Padding(padding: EdgeInsets.all(12), child: CountryPicker()),
        ),
      ),
    ));
    await tester.pumpAndSettle();

    expect(container.read(selectedCountryProvider), isNull);

    await tester.tap(find.byType(DropdownButton<String>));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Exit: Japan').last);
    await tester.pumpAndSettle();

    expect(container.read(selectedCountryProvider), 'JP');
  });
}
