// Idempotently provisions the import-set e2e fixture on the target instance so
// the `import-set load` / `import-set transform` scenarios can be validated
// end to end against a bare PDI without any manual setup.
//
// Creates (only what is missing):
//   * staging table  u_e2e_import  extending sys_import_set_row, with a
//     u_short_description string column;
//   * an active transform map  u_e2e_import_map:  u_e2e_import -> incident,
//     mapping u_short_description -> short_description with no coalesce (so a
//     transform always inserts a fresh, marked incident that teardown.js can
//     find and delete).
//
// Run via `snow-cli script run --file` (the /sys.scripts.do background-script
// endpoint), which needs an admin-like account — the same assumption the other
// mutating SNU/import scenarios already make. Prints a single JSON object; the
// platform prefixes it with "*** Script:", which snow-cli extracts. Logical
// provisioning problems (e.g. the physical table not being queryable) surface
// downstream as a real `import-set load` failure rather than a fake pass.
(function () {
  var STAGING = 'u_e2e_import';
  var COLUMN = 'u_short_description';
  var MAP_NAME = 'u_e2e_import_map';
  var out = { ok: false, staging_table: STAGING, created: [], warnings: [] };

  function findOne(table, field, value) {
    var gr = new GlideRecord(table);
    gr.addQuery(field, value);
    gr.setLimit(1);
    gr.query();
    return gr.next() ? gr : null;
  }

  // 1. The base import-set-row class must exist to extend from.
  var base = findOne('sys_db_object', 'name', 'sys_import_set_row');
  if (!base) {
    out.error = 'sys_import_set_row base table not found on this instance';
    gs.print(JSON.stringify(out));
    return;
  }

  // 2. Staging table (extends sys_import_set_row).
  if (!findOne('sys_db_object', 'name', STAGING)) {
    var t = new GlideRecord('sys_db_object');
    t.initialize();
    t.name = STAGING;
    t.label = 'snow-cli E2E Import';
    t.super_class = base.getUniqueValue();
    if (!t.insert()) {
      out.error = 'failed to create staging table ' + STAGING;
      gs.print(JSON.stringify(out));
      return;
    }
    out.created.push('table:' + STAGING);
  }

  // 3. String column on the staging table.
  var existingCol = new GlideRecord('sys_dictionary');
  existingCol.addQuery('name', STAGING);
  existingCol.addQuery('element', COLUMN);
  existingCol.setLimit(1);
  existingCol.query();
  if (!existingCol.next()) {
    var d = new GlideRecord('sys_dictionary');
    d.initialize();
    d.name = STAGING;
    d.element = COLUMN;
    d.column_label = 'Short description';
    d.internal_type = 'string';
    d.max_length = 100;
    if (d.insert()) {
      out.created.push('column:' + COLUMN);
    } else {
      out.warnings.push('failed to create column ' + COLUMN);
    }
  }

  // 4. Transform map -> incident.
  var mapId;
  var map = findOne('sys_transform_map', 'name', MAP_NAME);
  if (map) {
    mapId = map.getUniqueValue();
  } else {
    var m = new GlideRecord('sys_transform_map');
    m.initialize();
    m.name = MAP_NAME;
    m.source_table = STAGING;
    m.target_table = 'incident';
    m.active = true;
    m.run_business_rules = false;
    m.enforce_mandatory_fields = 'no';
    m.order = 100;
    mapId = m.insert();
    if (mapId) {
      out.created.push('transform_map:' + MAP_NAME);
    } else {
      out.warnings.push('failed to create transform map');
    }
  }

  // 5. Field entry: u_short_description -> short_description.
  if (mapId) {
    var existingEntry = new GlideRecord('sys_transform_entry');
    existingEntry.addQuery('map', mapId);
    existingEntry.addQuery('source_field', COLUMN);
    existingEntry.setLimit(1);
    existingEntry.query();
    if (!existingEntry.next()) {
      var e = new GlideRecord('sys_transform_entry');
      e.initialize();
      e.map = mapId;
      e.source_field = COLUMN;
      e.target_field = 'short_description';
      e.coalesce = false;
      if (e.insert()) {
        out.created.push('transform_entry:' + COLUMN + '->short_description');
      } else {
        out.warnings.push('failed to create transform entry');
      }
    }
  }

  // 6. Confirm the physical staging table is actually queryable.
  out.staging_table_valid = new GlideRecord(STAGING).isValid();
  if (!out.staging_table_valid) {
    out.warnings.push(
      'staging table is not queryable yet — scripted table provisioning may be ' +
        'async or restricted on this instance'
    );
  }

  out.ok = out.staging_table_valid && out.warnings.length === 0;
  gs.print(JSON.stringify(out));
})();
