// Best-effort teardown for the import-set e2e fixture created by provision.js.
// Idempotent: anything already gone is simply skipped, so it is safe to run
// even if a prior run half-provisioned or a previous teardown already ran.
//
// Deletes, in dependency order:
//   1. incidents the demo transform inserted (marked via short_description);
//   2. import sets staged into the staging table (rows cascade) + any orphan
//      staging rows;
//   3. the transform map and its field entries;
//   4. the staging table's dictionary columns and the table itself.
//
// Run via `snow-cli script run --file` as a scenario cleanup step; cleanup
// steps are allowed to fail without failing the scenario. Prints a JSON
// summary (platform-prefixed with "*** Script:").
(function () {
  var STAGING = 'u_e2e_import';
  var MAP_NAME = 'u_e2e_import_map';
  var out = { deleted: {}, warnings: [] };

  function purge(table, encodedQuery, label) {
    var gr = new GlideRecord(table);
    if (!gr.isValid()) {
      return;
    }
    if (encodedQuery) {
      gr.addEncodedQuery(encodedQuery);
    }
    gr.query();
    var count = 0;
    while (gr.next()) {
      gr.deleteRecord();
      count++;
    }
    out.deleted[label] = count;
  }

  // 1. Incidents created by the demo transform (both load/transform markers).
  purge('incident', 'short_descriptionLIKEsnow-cli e2e scenario: import-set', 'incidents');

  // 2. Import sets targeting the staging table (their rows cascade), then any
  //    remaining staging rows.
  purge('sys_import_set', 'table_name=' + STAGING, 'import_sets');
  purge(STAGING, '', 'staging_rows');

  // 3. Transform map + entries.
  var entryCount = 0;
  var mapCount = 0;
  var map = new GlideRecord('sys_transform_map');
  map.addQuery('name', MAP_NAME);
  map.query();
  while (map.next()) {
    var entries = new GlideRecord('sys_transform_entry');
    entries.addQuery('map', map.getUniqueValue());
    entries.query();
    while (entries.next()) {
      entries.deleteRecord();
      entryCount++;
    }
    map.deleteRecord();
    mapCount++;
  }
  out.deleted.transform_entries = entryCount;
  out.deleted.transform_maps = mapCount;

  // 4. Drop the staging table (this also removes its dictionary rows), then
  //    sweep any dictionary leftovers as a safety net.
  purge('sys_db_object', 'name=' + STAGING, 'tables');
  purge('sys_dictionary', 'name=' + STAGING, 'dictionary_rows');

  out.ok = true;
  gs.print(JSON.stringify(out));
})();
