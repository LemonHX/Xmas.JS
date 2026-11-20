(function () {
  // The algorithms are in Rust; this is the shape Node gives them. Each call
  // turns its arguments into strings first, as Node does, and join and resolve
  // hand several paths over in one call separated by NUL, which no path may
  // contain.
  const SEP = __path_sep();
  const DELIMITER = __path_delimiter();
  const NUL = '\u0000';
  const SPECIAL = '.+^$()|[]{}\\';

  const both = (parts) => parts.map((part) => String(part)).join(NUL);

  const globSource = (glob) => {
    let source = '';
    let index = 0;
    while (index < glob.length) {
      const character = glob[index];
      if (character === '*' && glob[index + 1] === '*') {
        source += '.*';
        index += 2;
        continue;
      }
      if (character === '*') {
        source += '[^/]*';
        index += 1;
        continue;
      }
      if (character === '?') {
        source += '.';
        index += 1;
        continue;
      }
      source += SPECIAL.indexOf(character) >= 0 ? '\\' + character : character;
      index += 1;
    }
    return source;
  };

  const path = {
    sep: SEP,
    delimiter: DELIMITER,
    dirname: (value) => __path_dirname(String(value)),
    basename: (value, suffix) => __path_basename(String(value), suffix === undefined ? '' : String(suffix)),
    extname: (value) => __path_extname(String(value)),
    join: function (...parts) {
      return __path_join(both(parts));
    },
    resolve: function (...parts) {
      return __path_resolve(both(parts));
    },
    relative: (from, to) => __path_relative(String(from), String(to)),
    normalize: (value) => __path_normalize(String(value)),
    isAbsolute: (value) => __path_is_absolute(String(value)),
    parse: (value) => JSON.parse(__path_parse(String(value))),
    format: (shape) => {
      if (!shape || typeof shape !== 'object') throw new TypeError('The pathObject argument must be of type object');
      const pick = (key) => (shape[key] === undefined || shape[key] === null ? '' : String(shape[key]));
      return __path_format(pick('dir'), pick('root'), pick('base'), pick('name'), pick('ext'));
    },
    toNamespacedPath: (value) => String(value),
    matchesGlob: (name, pattern) => new RegExp('^' + globSource(String(pattern)) + '$').test(String(name)),
  };

  // Only the flavour of the platform this was built for exists. Both names point
  // at it, so code that reaches for the other one gets the host's rules instead
  // of an undefined crash.
  path.posix = path;
  path.win32 = path;

  globalThis.path = path;
})();
