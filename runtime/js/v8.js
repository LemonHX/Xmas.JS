(function () {
  // node:v8, the parts libraries reach for. serialize/deserialize go through JSON (so they carry
  // plain data, not Map/Set/cycles); the heap numbers are zeros, there is no v8 here.
  const serialize = (value) => Buffer.from(JSON.stringify(value === undefined ? null : value));
  const deserialize = (buffer) => JSON.parse(Buffer.from(buffer).toString());
  const zero = () => ({ total_heap_size: 0, total_heap_size_executable: 0, total_physical_size: 0, total_available_size: 0, used_heap_size: 0, heap_size_limit: 0, malloced_memory: 0, peak_malloced_memory: 0, does_zap_garbage: 0, number_of_native_contexts: 0, number_of_detached_contexts: 0 });
  globalThis.__v8_module = { serialize, deserialize, getHeapStatistics: zero, setFlagsFromString() {}, cachedDataVersionTag: () => 0 };
})();
