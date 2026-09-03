#!/usr/bin/env node
import { createRequire } from 'node:module'; const require = createRequire(import.meta.url);
var __create = Object.create;
var __defProp = Object.defineProperty;
var __getOwnPropDesc = Object.getOwnPropertyDescriptor;
var __getOwnPropNames = Object.getOwnPropertyNames;
var __getProtoOf = Object.getPrototypeOf;
var __hasOwnProp = Object.prototype.hasOwnProperty;
var __require = /* @__PURE__ */ ((x) => typeof require !== "undefined" ? require : typeof Proxy !== "undefined" ? new Proxy(x, {
  get: (a, b) => (typeof require !== "undefined" ? require : a)[b]
}) : x)(function(x) {
  if (typeof require !== "undefined") return require.apply(this, arguments);
  throw Error('Dynamic require of "' + x + '" is not supported');
});
var __commonJS = (cb, mod) => function __require2() {
  try {
    return mod || (0, cb[__getOwnPropNames(cb)[0]])((mod = { exports: {} }).exports, mod), mod.exports;
  } catch (e) {
    throw mod = 0, e;
  }
};
var __copyProps = (to, from, except, desc) => {
  if (from && typeof from === "object" || typeof from === "function") {
    for (let key of __getOwnPropNames(from))
      if (!__hasOwnProp.call(to, key) && key !== except)
        __defProp(to, key, { get: () => from[key], enumerable: !(desc = __getOwnPropDesc(from, key)) || desc.enumerable });
  }
  return to;
};
var __toESM = (mod, isNodeMode, target) => (target = mod != null ? __create(__getProtoOf(mod)) : {}, __copyProps(
  // If the importer is in node compatibility mode or this is not an ESM
  // file that has been converted to a CommonJS file using a Babel-
  // compatible transform (i.e. "__esModule" has not been set), then set
  // "default" to the CommonJS "module.exports" for node compatibility.
  isNodeMode || !mod || !mod.__esModule ? __defProp(target, "default", { value: mod, enumerable: true }) : target,
  mod
));

// node_modules/yaml/dist/nodes/identity.js
var require_identity = __commonJS({
  "node_modules/yaml/dist/nodes/identity.js"(exports) {
    "use strict";
    var ALIAS = /* @__PURE__ */ Symbol.for("yaml.alias");
    var DOC = /* @__PURE__ */ Symbol.for("yaml.document");
    var MAP = /* @__PURE__ */ Symbol.for("yaml.map");
    var PAIR = /* @__PURE__ */ Symbol.for("yaml.pair");
    var SCALAR = /* @__PURE__ */ Symbol.for("yaml.scalar");
    var SEQ = /* @__PURE__ */ Symbol.for("yaml.seq");
    var NODE_TYPE = /* @__PURE__ */ Symbol.for("yaml.node.type");
    var isAlias = (node) => !!node && typeof node === "object" && node[NODE_TYPE] === ALIAS;
    var isDocument = (node) => !!node && typeof node === "object" && node[NODE_TYPE] === DOC;
    var isMap = (node) => !!node && typeof node === "object" && node[NODE_TYPE] === MAP;
    var isPair = (node) => !!node && typeof node === "object" && node[NODE_TYPE] === PAIR;
    var isScalar = (node) => !!node && typeof node === "object" && node[NODE_TYPE] === SCALAR;
    var isSeq = (node) => !!node && typeof node === "object" && node[NODE_TYPE] === SEQ;
    function isCollection(node) {
      if (node && typeof node === "object")
        switch (node[NODE_TYPE]) {
          case MAP:
          case SEQ:
            return true;
        }
      return false;
    }
    function isNode(node) {
      if (node && typeof node === "object")
        switch (node[NODE_TYPE]) {
          case ALIAS:
          case MAP:
          case SCALAR:
          case SEQ:
            return true;
        }
      return false;
    }
    var hasAnchor = (node) => (isScalar(node) || isCollection(node)) && !!node.anchor;
    exports.ALIAS = ALIAS;
    exports.DOC = DOC;
    exports.MAP = MAP;
    exports.NODE_TYPE = NODE_TYPE;
    exports.PAIR = PAIR;
    exports.SCALAR = SCALAR;
    exports.SEQ = SEQ;
    exports.hasAnchor = hasAnchor;
    exports.isAlias = isAlias;
    exports.isCollection = isCollection;
    exports.isDocument = isDocument;
    exports.isMap = isMap;
    exports.isNode = isNode;
    exports.isPair = isPair;
    exports.isScalar = isScalar;
    exports.isSeq = isSeq;
  }
});

// node_modules/yaml/dist/visit.js
var require_visit = __commonJS({
  "node_modules/yaml/dist/visit.js"(exports) {
    "use strict";
    var identity = require_identity();
    var BREAK = /* @__PURE__ */ Symbol("break visit");
    var SKIP = /* @__PURE__ */ Symbol("skip children");
    var REMOVE = /* @__PURE__ */ Symbol("remove node");
    function visit(node, visitor) {
      const visitor_ = initVisitor(visitor);
      if (identity.isDocument(node)) {
        const cd = visit_(null, node.contents, visitor_, Object.freeze([node]));
        if (cd === REMOVE)
          node.contents = null;
      } else
        visit_(null, node, visitor_, Object.freeze([]));
    }
    visit.BREAK = BREAK;
    visit.SKIP = SKIP;
    visit.REMOVE = REMOVE;
    function visit_(key, node, visitor, path) {
      const ctrl = callVisitor(key, node, visitor, path);
      if (identity.isNode(ctrl) || identity.isPair(ctrl)) {
        replaceNode(key, path, ctrl);
        return visit_(key, ctrl, visitor, path);
      }
      if (typeof ctrl !== "symbol") {
        if (identity.isCollection(node)) {
          path = Object.freeze(path.concat(node));
          for (let i = 0; i < node.items.length; ++i) {
            const ci = visit_(i, node.items[i], visitor, path);
            if (typeof ci === "number")
              i = ci - 1;
            else if (ci === BREAK)
              return BREAK;
            else if (ci === REMOVE) {
              node.items.splice(i, 1);
              i -= 1;
            }
          }
        } else if (identity.isPair(node)) {
          path = Object.freeze(path.concat(node));
          const ck = visit_("key", node.key, visitor, path);
          if (ck === BREAK)
            return BREAK;
          else if (ck === REMOVE)
            node.key = null;
          const cv = visit_("value", node.value, visitor, path);
          if (cv === BREAK)
            return BREAK;
          else if (cv === REMOVE)
            node.value = null;
        }
      }
      return ctrl;
    }
    async function visitAsync(node, visitor) {
      const visitor_ = initVisitor(visitor);
      if (identity.isDocument(node)) {
        const cd = await visitAsync_(null, node.contents, visitor_, Object.freeze([node]));
        if (cd === REMOVE)
          node.contents = null;
      } else
        await visitAsync_(null, node, visitor_, Object.freeze([]));
    }
    visitAsync.BREAK = BREAK;
    visitAsync.SKIP = SKIP;
    visitAsync.REMOVE = REMOVE;
    async function visitAsync_(key, node, visitor, path) {
      const ctrl = await callVisitor(key, node, visitor, path);
      if (identity.isNode(ctrl) || identity.isPair(ctrl)) {
        replaceNode(key, path, ctrl);
        return visitAsync_(key, ctrl, visitor, path);
      }
      if (typeof ctrl !== "symbol") {
        if (identity.isCollection(node)) {
          path = Object.freeze(path.concat(node));
          for (let i = 0; i < node.items.length; ++i) {
            const ci = await visitAsync_(i, node.items[i], visitor, path);
            if (typeof ci === "number")
              i = ci - 1;
            else if (ci === BREAK)
              return BREAK;
            else if (ci === REMOVE) {
              node.items.splice(i, 1);
              i -= 1;
            }
          }
        } else if (identity.isPair(node)) {
          path = Object.freeze(path.concat(node));
          const ck = await visitAsync_("key", node.key, visitor, path);
          if (ck === BREAK)
            return BREAK;
          else if (ck === REMOVE)
            node.key = null;
          const cv = await visitAsync_("value", node.value, visitor, path);
          if (cv === BREAK)
            return BREAK;
          else if (cv === REMOVE)
            node.value = null;
        }
      }
      return ctrl;
    }
    function initVisitor(visitor) {
      if (typeof visitor === "object" && (visitor.Collection || visitor.Node || visitor.Value)) {
        return Object.assign({
          Alias: visitor.Node,
          Map: visitor.Node,
          Scalar: visitor.Node,
          Seq: visitor.Node
        }, visitor.Value && {
          Map: visitor.Value,
          Scalar: visitor.Value,
          Seq: visitor.Value
        }, visitor.Collection && {
          Map: visitor.Collection,
          Seq: visitor.Collection
        }, visitor);
      }
      return visitor;
    }
    function callVisitor(key, node, visitor, path) {
      if (typeof visitor === "function")
        return visitor(key, node, path);
      if (identity.isMap(node))
        return visitor.Map?.(key, node, path);
      if (identity.isSeq(node))
        return visitor.Seq?.(key, node, path);
      if (identity.isPair(node))
        return visitor.Pair?.(key, node, path);
      if (identity.isScalar(node))
        return visitor.Scalar?.(key, node, path);
      if (identity.isAlias(node))
        return visitor.Alias?.(key, node, path);
      return void 0;
    }
    function replaceNode(key, path, node) {
      const parent = path[path.length - 1];
      if (identity.isCollection(parent)) {
        parent.items[key] = node;
      } else if (identity.isPair(parent)) {
        if (key === "key")
          parent.key = node;
        else
          parent.value = node;
      } else if (identity.isDocument(parent)) {
        parent.contents = node;
      } else {
        const pt = identity.isAlias(parent) ? "alias" : "scalar";
        throw new Error(`Cannot replace node with ${pt} parent`);
      }
    }
    exports.visit = visit;
    exports.visitAsync = visitAsync;
  }
});

// node_modules/yaml/dist/doc/directives.js
var require_directives = __commonJS({
  "node_modules/yaml/dist/doc/directives.js"(exports) {
    "use strict";
    var identity = require_identity();
    var visit = require_visit();
    var escapeChars = {
      "!": "%21",
      ",": "%2C",
      "[": "%5B",
      "]": "%5D",
      "{": "%7B",
      "}": "%7D"
    };
    var escapeTagName = (tn) => tn.replace(/[!,[\]{}]/g, (ch) => escapeChars[ch]);
    var Directives = class _Directives {
      constructor(yaml, tags) {
        this.docStart = null;
        this.docEnd = false;
        this.yaml = Object.assign({}, _Directives.defaultYaml, yaml);
        this.tags = Object.assign({}, _Directives.defaultTags, tags);
      }
      clone() {
        const copy = new _Directives(this.yaml, this.tags);
        copy.docStart = this.docStart;
        return copy;
      }
      /**
       * During parsing, get a Directives instance for the current document and
       * update the stream state according to the current version's spec.
       */
      atDocument() {
        const res = new _Directives(this.yaml, this.tags);
        switch (this.yaml.version) {
          case "1.1":
            this.atNextDocument = true;
            break;
          case "1.2":
            this.atNextDocument = false;
            this.yaml = {
              explicit: _Directives.defaultYaml.explicit,
              version: "1.2"
            };
            this.tags = Object.assign({}, _Directives.defaultTags);
            break;
        }
        return res;
      }
      /**
       * @param onError - May be called even if the action was successful
       * @returns `true` on success
       */
      add(line, onError) {
        if (this.atNextDocument) {
          this.yaml = { explicit: _Directives.defaultYaml.explicit, version: "1.1" };
          this.tags = Object.assign({}, _Directives.defaultTags);
          this.atNextDocument = false;
        }
        const parts = line.trim().split(/[ \t]+/);
        const name = parts.shift();
        switch (name) {
          case "%TAG": {
            if (parts.length !== 2) {
              onError(0, "%TAG directive should contain exactly two parts");
              if (parts.length < 2)
                return false;
            }
            const [handle, prefix] = parts;
            this.tags[handle] = prefix;
            return true;
          }
          case "%YAML": {
            this.yaml.explicit = true;
            if (parts.length !== 1) {
              onError(0, "%YAML directive should contain exactly one part");
              return false;
            }
            const [version] = parts;
            if (version === "1.1" || version === "1.2") {
              this.yaml.version = version;
              return true;
            } else {
              const isValid = /^\d+\.\d+$/.test(version);
              onError(6, `Unsupported YAML version ${version}`, isValid);
              return false;
            }
          }
          default:
            onError(0, `Unknown directive ${name}`, true);
            return false;
        }
      }
      /**
       * Resolves a tag, matching handles to those defined in %TAG directives.
       *
       * @returns Resolved tag, which may also be the non-specific tag `'!'` or a
       *   `'!local'` tag, or `null` if unresolvable.
       */
      tagName(source, onError) {
        if (source === "!")
          return "!";
        if (source[0] !== "!") {
          onError(`Not a valid tag: ${source}`);
          return null;
        }
        if (source[1] === "<") {
          const verbatim = source.slice(2, -1);
          if (verbatim === "!" || verbatim === "!!") {
            onError(`Verbatim tags aren't resolved, so ${source} is invalid.`);
            return null;
          }
          if (source[source.length - 1] !== ">")
            onError("Verbatim tags must end with a >");
          return verbatim;
        }
        const [, handle, suffix] = source.match(/^(.*!)([^!]*)$/s);
        if (!suffix)
          onError(`The ${source} tag has no suffix`);
        const prefix = this.tags[handle];
        if (prefix) {
          try {
            return prefix + decodeURIComponent(suffix);
          } catch (error) {
            onError(String(error));
            return null;
          }
        }
        if (handle === "!")
          return source;
        onError(`Could not resolve tag: ${source}`);
        return null;
      }
      /**
       * Given a fully resolved tag, returns its printable string form,
       * taking into account current tag prefixes and defaults.
       */
      tagString(tag) {
        for (const [handle, prefix] of Object.entries(this.tags)) {
          if (tag.startsWith(prefix))
            return handle + escapeTagName(tag.substring(prefix.length));
        }
        return tag[0] === "!" ? tag : `!<${tag}>`;
      }
      toString(doc) {
        const lines = this.yaml.explicit ? [`%YAML ${this.yaml.version || "1.2"}`] : [];
        const tagEntries = Object.entries(this.tags);
        let tagNames;
        if (doc && tagEntries.length > 0 && identity.isNode(doc.contents)) {
          const tags = {};
          visit.visit(doc.contents, (_key, node) => {
            if (identity.isNode(node) && node.tag)
              tags[node.tag] = true;
          });
          tagNames = Object.keys(tags);
        } else
          tagNames = [];
        for (const [handle, prefix] of tagEntries) {
          if (handle === "!!" && prefix === "tag:yaml.org,2002:")
            continue;
          if (!doc || tagNames.some((tn) => tn.startsWith(prefix)))
            lines.push(`%TAG ${handle} ${prefix}`);
        }
        return lines.join("\n");
      }
    };
    Directives.defaultYaml = { explicit: false, version: "1.2" };
    Directives.defaultTags = { "!!": "tag:yaml.org,2002:" };
    exports.Directives = Directives;
  }
});

// node_modules/yaml/dist/doc/anchors.js
var require_anchors = __commonJS({
  "node_modules/yaml/dist/doc/anchors.js"(exports) {
    "use strict";
    var identity = require_identity();
    var visit = require_visit();
    function anchorIsValid(anchor) {
      if (/[\x00-\x19\s,[\]{}]/.test(anchor)) {
        const sa = JSON.stringify(anchor);
        const msg = `Anchor must not contain whitespace or control characters: ${sa}`;
        throw new Error(msg);
      }
      return true;
    }
    function anchorNames(root) {
      const anchors = /* @__PURE__ */ new Set();
      visit.visit(root, {
        Value(_key, node) {
          if (node.anchor)
            anchors.add(node.anchor);
        }
      });
      return anchors;
    }
    function findNewAnchor(prefix, exclude) {
      for (let i = 1; true; ++i) {
        const name = `${prefix}${i}`;
        if (!exclude.has(name))
          return name;
      }
    }
    function createNodeAnchors(doc, prefix) {
      const aliasObjects = [];
      const sourceObjects = /* @__PURE__ */ new Map();
      let prevAnchors = null;
      return {
        onAnchor: (source) => {
          aliasObjects.push(source);
          prevAnchors ?? (prevAnchors = anchorNames(doc));
          const anchor = findNewAnchor(prefix, prevAnchors);
          prevAnchors.add(anchor);
          return anchor;
        },
        /**
         * With circular references, the source node is only resolved after all
         * of its child nodes are. This is why anchors are set only after all of
         * the nodes have been created.
         */
        setAnchors: () => {
          for (const source of aliasObjects) {
            const ref = sourceObjects.get(source);
            if (typeof ref === "object" && ref.anchor && (identity.isScalar(ref.node) || identity.isCollection(ref.node))) {
              ref.node.anchor = ref.anchor;
            } else {
              const error = new Error("Failed to resolve repeated object (this should not happen)");
              error.source = source;
              throw error;
            }
          }
        },
        sourceObjects
      };
    }
    exports.anchorIsValid = anchorIsValid;
    exports.anchorNames = anchorNames;
    exports.createNodeAnchors = createNodeAnchors;
    exports.findNewAnchor = findNewAnchor;
  }
});

// node_modules/yaml/dist/doc/applyReviver.js
var require_applyReviver = __commonJS({
  "node_modules/yaml/dist/doc/applyReviver.js"(exports) {
    "use strict";
    function applyReviver(reviver, obj, key, val) {
      if (val && typeof val === "object") {
        if (Array.isArray(val)) {
          for (let i = 0, len = val.length; i < len; ++i) {
            const v0 = val[i];
            const v1 = applyReviver(reviver, val, String(i), v0);
            if (v1 === void 0)
              delete val[i];
            else if (v1 !== v0)
              val[i] = v1;
          }
        } else if (val instanceof Map) {
          for (const k of Array.from(val.keys())) {
            const v0 = val.get(k);
            const v1 = applyReviver(reviver, val, k, v0);
            if (v1 === void 0)
              val.delete(k);
            else if (v1 !== v0)
              val.set(k, v1);
          }
        } else if (val instanceof Set) {
          for (const v0 of Array.from(val)) {
            const v1 = applyReviver(reviver, val, v0, v0);
            if (v1 === void 0)
              val.delete(v0);
            else if (v1 !== v0) {
              val.delete(v0);
              val.add(v1);
            }
          }
        } else {
          for (const [k, v0] of Object.entries(val)) {
            const v1 = applyReviver(reviver, val, k, v0);
            if (v1 === void 0)
              delete val[k];
            else if (v1 !== v0)
              val[k] = v1;
          }
        }
      }
      return reviver.call(obj, key, val);
    }
    exports.applyReviver = applyReviver;
  }
});

// node_modules/yaml/dist/nodes/toJS.js
var require_toJS = __commonJS({
  "node_modules/yaml/dist/nodes/toJS.js"(exports) {
    "use strict";
    var identity = require_identity();
    function toJS(value, arg, ctx) {
      if (Array.isArray(value))
        return value.map((v, i) => toJS(v, String(i), ctx));
      if (value && typeof value.toJSON === "function") {
        if (!ctx || !identity.hasAnchor(value))
          return value.toJSON(arg, ctx);
        const data = { aliasCount: 0, count: 1, res: void 0 };
        ctx.anchors.set(value, data);
        ctx.onCreate = (res2) => {
          data.res = res2;
          delete ctx.onCreate;
        };
        const res = value.toJSON(arg, ctx);
        if (ctx.onCreate)
          ctx.onCreate(res);
        return res;
      }
      if (typeof value === "bigint" && !ctx?.keep)
        return Number(value);
      return value;
    }
    exports.toJS = toJS;
  }
});

// node_modules/yaml/dist/nodes/Node.js
var require_Node = __commonJS({
  "node_modules/yaml/dist/nodes/Node.js"(exports) {
    "use strict";
    var applyReviver = require_applyReviver();
    var identity = require_identity();
    var toJS = require_toJS();
    var NodeBase = class {
      constructor(type) {
        Object.defineProperty(this, identity.NODE_TYPE, { value: type });
      }
      /** Create a copy of this node.  */
      clone() {
        const copy = Object.create(Object.getPrototypeOf(this), Object.getOwnPropertyDescriptors(this));
        if (this.range)
          copy.range = this.range.slice();
        return copy;
      }
      /** A plain JavaScript representation of this node. */
      toJS(doc, { mapAsMap, maxAliasCount, onAnchor, reviver } = {}) {
        if (!identity.isDocument(doc))
          throw new TypeError("A document argument is required");
        const ctx = {
          anchors: /* @__PURE__ */ new Map(),
          doc,
          keep: true,
          mapAsMap: mapAsMap === true,
          mapKeyWarned: false,
          maxAliasCount: typeof maxAliasCount === "number" ? maxAliasCount : 100
        };
        const res = toJS.toJS(this, "", ctx);
        if (typeof onAnchor === "function")
          for (const { count, res: res2 } of ctx.anchors.values())
            onAnchor(res2, count);
        return typeof reviver === "function" ? applyReviver.applyReviver(reviver, { "": res }, "", res) : res;
      }
    };
    exports.NodeBase = NodeBase;
  }
});

// node_modules/yaml/dist/nodes/Alias.js
var require_Alias = __commonJS({
  "node_modules/yaml/dist/nodes/Alias.js"(exports) {
    "use strict";
    var anchors = require_anchors();
    var visit = require_visit();
    var identity = require_identity();
    var Node = require_Node();
    var toJS = require_toJS();
    var Alias = class extends Node.NodeBase {
      constructor(source) {
        super(identity.ALIAS);
        this.source = source;
        Object.defineProperty(this, "tag", {
          set() {
            throw new Error("Alias nodes cannot have tags");
          }
        });
      }
      /**
       * Resolve the value of this alias within `doc`, finding the last
       * instance of the `source` anchor before this node.
       */
      resolve(doc, ctx) {
        if (ctx?.maxAliasCount === 0)
          throw new ReferenceError("Alias resolution is disabled");
        let nodes;
        if (ctx?.aliasResolveCache) {
          nodes = ctx.aliasResolveCache;
        } else {
          nodes = [];
          visit.visit(doc, {
            Node: (_key, node) => {
              if (identity.isAlias(node) || identity.hasAnchor(node))
                nodes.push(node);
            }
          });
          if (ctx)
            ctx.aliasResolveCache = nodes;
        }
        let found = void 0;
        for (const node of nodes) {
          if (node === this)
            break;
          if (node.anchor === this.source)
            found = node;
        }
        return found;
      }
      toJSON(_arg, ctx) {
        if (!ctx)
          return { source: this.source };
        const { anchors: anchors2, doc, maxAliasCount } = ctx;
        const source = this.resolve(doc, ctx);
        if (!source) {
          const msg = `Unresolved alias (the anchor must be set before the alias): ${this.source}`;
          throw new ReferenceError(msg);
        }
        let data = anchors2.get(source);
        if (!data) {
          toJS.toJS(source, null, ctx);
          data = anchors2.get(source);
        }
        if (data?.res === void 0) {
          const msg = "This should not happen: Alias anchor was not resolved?";
          throw new ReferenceError(msg);
        }
        if (maxAliasCount >= 0) {
          data.count += 1;
          if (data.aliasCount === 0)
            data.aliasCount = getAliasCount(doc, source, anchors2);
          if (data.count * data.aliasCount > maxAliasCount) {
            const msg = "Excessive alias count indicates a resource exhaustion attack";
            throw new ReferenceError(msg);
          }
        }
        return data.res;
      }
      toString(ctx, _onComment, _onChompKeep) {
        const src = `*${this.source}`;
        if (ctx) {
          anchors.anchorIsValid(this.source);
          if (ctx.options.verifyAliasOrder && !ctx.anchors.has(this.source)) {
            const msg = `Unresolved alias (the anchor must be set before the alias): ${this.source}`;
            throw new Error(msg);
          }
          if (ctx.implicitKey)
            return `${src} `;
        }
        return src;
      }
    };
    function getAliasCount(doc, node, anchors2) {
      if (identity.isAlias(node)) {
        const source = node.resolve(doc);
        const anchor = anchors2 && source && anchors2.get(source);
        return anchor ? anchor.count * anchor.aliasCount : 0;
      } else if (identity.isCollection(node)) {
        let count = 0;
        for (const item of node.items) {
          const c = getAliasCount(doc, item, anchors2);
          if (c > count)
            count = c;
        }
        return count;
      } else if (identity.isPair(node)) {
        const kc = getAliasCount(doc, node.key, anchors2);
        const vc = getAliasCount(doc, node.value, anchors2);
        return Math.max(kc, vc);
      }
      return 1;
    }
    exports.Alias = Alias;
  }
});

// node_modules/yaml/dist/nodes/Scalar.js
var require_Scalar = __commonJS({
  "node_modules/yaml/dist/nodes/Scalar.js"(exports) {
    "use strict";
    var identity = require_identity();
    var Node = require_Node();
    var toJS = require_toJS();
    var isScalarValue = (value) => !value || typeof value !== "function" && typeof value !== "object";
    var Scalar = class extends Node.NodeBase {
      constructor(value) {
        super(identity.SCALAR);
        this.value = value;
      }
      toJSON(arg, ctx) {
        return ctx?.keep ? this.value : toJS.toJS(this.value, arg, ctx);
      }
      toString() {
        return String(this.value);
      }
    };
    Scalar.BLOCK_FOLDED = "BLOCK_FOLDED";
    Scalar.BLOCK_LITERAL = "BLOCK_LITERAL";
    Scalar.PLAIN = "PLAIN";
    Scalar.QUOTE_DOUBLE = "QUOTE_DOUBLE";
    Scalar.QUOTE_SINGLE = "QUOTE_SINGLE";
    exports.Scalar = Scalar;
    exports.isScalarValue = isScalarValue;
  }
});

// node_modules/yaml/dist/doc/createNode.js
var require_createNode = __commonJS({
  "node_modules/yaml/dist/doc/createNode.js"(exports) {
    "use strict";
    var Alias = require_Alias();
    var identity = require_identity();
    var Scalar = require_Scalar();
    var defaultTagPrefix = "tag:yaml.org,2002:";
    function findTagObject(value, tagName, tags) {
      if (tagName) {
        const match = tags.filter((t) => t.tag === tagName);
        const tagObj = match.find((t) => !t.format) ?? match[0];
        if (!tagObj)
          throw new Error(`Tag ${tagName} not found`);
        return tagObj;
      }
      return tags.find((t) => t.identify?.(value) && !t.format);
    }
    function createNode(value, tagName, ctx) {
      if (identity.isDocument(value))
        value = value.contents;
      if (identity.isNode(value))
        return value;
      if (identity.isPair(value)) {
        const map = ctx.schema[identity.MAP].createNode?.(ctx.schema, null, ctx);
        map.items.push(value);
        return map;
      }
      if (value instanceof String || value instanceof Number || value instanceof Boolean || typeof BigInt !== "undefined" && value instanceof BigInt) {
        value = value.valueOf();
      }
      const { aliasDuplicateObjects, onAnchor, onTagObj, schema, sourceObjects } = ctx;
      let ref = void 0;
      if (aliasDuplicateObjects && value && typeof value === "object") {
        ref = sourceObjects.get(value);
        if (ref) {
          ref.anchor ?? (ref.anchor = onAnchor(value));
          return new Alias.Alias(ref.anchor);
        } else {
          ref = { anchor: null, node: null };
          sourceObjects.set(value, ref);
        }
      }
      if (tagName?.startsWith("!!"))
        tagName = defaultTagPrefix + tagName.slice(2);
      let tagObj = findTagObject(value, tagName, schema.tags);
      if (!tagObj) {
        if (value && typeof value.toJSON === "function") {
          value = value.toJSON();
        }
        if (!value || typeof value !== "object") {
          const node2 = new Scalar.Scalar(value);
          if (ref)
            ref.node = node2;
          return node2;
        }
        tagObj = value instanceof Map ? schema[identity.MAP] : Symbol.iterator in Object(value) ? schema[identity.SEQ] : schema[identity.MAP];
      }
      if (onTagObj) {
        onTagObj(tagObj);
        delete ctx.onTagObj;
      }
      const node = tagObj?.createNode ? tagObj.createNode(ctx.schema, value, ctx) : typeof tagObj?.nodeClass?.from === "function" ? tagObj.nodeClass.from(ctx.schema, value, ctx) : new Scalar.Scalar(value);
      if (tagName)
        node.tag = tagName;
      else if (!tagObj.default)
        node.tag = tagObj.tag;
      if (ref)
        ref.node = node;
      return node;
    }
    exports.createNode = createNode;
  }
});

// node_modules/yaml/dist/nodes/Collection.js
var require_Collection = __commonJS({
  "node_modules/yaml/dist/nodes/Collection.js"(exports) {
    "use strict";
    var createNode = require_createNode();
    var identity = require_identity();
    var Node = require_Node();
    function collectionFromPath(schema, path, value) {
      let v = value;
      for (let i = path.length - 1; i >= 0; --i) {
        const k = path[i];
        if (typeof k === "number" && Number.isInteger(k) && k >= 0) {
          const a = [];
          a[k] = v;
          v = a;
        } else {
          v = /* @__PURE__ */ new Map([[k, v]]);
        }
      }
      return createNode.createNode(v, void 0, {
        aliasDuplicateObjects: false,
        keepUndefined: false,
        onAnchor: () => {
          throw new Error("This should not happen, please report a bug.");
        },
        schema,
        sourceObjects: /* @__PURE__ */ new Map()
      });
    }
    var isEmptyPath = (path) => path == null || typeof path === "object" && !!path[Symbol.iterator]().next().done;
    var Collection = class extends Node.NodeBase {
      constructor(type, schema) {
        super(type);
        Object.defineProperty(this, "schema", {
          value: schema,
          configurable: true,
          enumerable: false,
          writable: true
        });
      }
      /**
       * Create a copy of this collection.
       *
       * @param schema - If defined, overwrites the original's schema
       */
      clone(schema) {
        const copy = Object.create(Object.getPrototypeOf(this), Object.getOwnPropertyDescriptors(this));
        if (schema)
          copy.schema = schema;
        copy.items = copy.items.map((it) => identity.isNode(it) || identity.isPair(it) ? it.clone(schema) : it);
        if (this.range)
          copy.range = this.range.slice();
        return copy;
      }
      /**
       * Adds a value to the collection. For `!!map` and `!!omap` the value must
       * be a Pair instance or a `{ key, value }` object, which may not have a key
       * that already exists in the map.
       */
      addIn(path, value) {
        if (isEmptyPath(path))
          this.add(value);
        else {
          const [key, ...rest] = path;
          const node = this.get(key, true);
          if (identity.isCollection(node))
            node.addIn(rest, value);
          else if (node === void 0 && this.schema)
            this.set(key, collectionFromPath(this.schema, rest, value));
          else
            throw new Error(`Expected YAML collection at ${key}. Remaining path: ${rest}`);
        }
      }
      /**
       * Removes a value from the collection.
       * @returns `true` if the item was found and removed.
       */
      deleteIn(path) {
        const [key, ...rest] = path;
        if (rest.length === 0)
          return this.delete(key);
        const node = this.get(key, true);
        if (identity.isCollection(node))
          return node.deleteIn(rest);
        else
          throw new Error(`Expected YAML collection at ${key}. Remaining path: ${rest}`);
      }
      /**
       * Returns item at `key`, or `undefined` if not found. By default unwraps
       * scalar values from their surrounding node; to disable set `keepScalar` to
       * `true` (collections are always returned intact).
       */
      getIn(path, keepScalar) {
        const [key, ...rest] = path;
        const node = this.get(key, true);
        if (rest.length === 0)
          return !keepScalar && identity.isScalar(node) ? node.value : node;
        else
          return identity.isCollection(node) ? node.getIn(rest, keepScalar) : void 0;
      }
      hasAllNullValues(allowScalar) {
        return this.items.every((node) => {
          if (!identity.isPair(node))
            return false;
          const n = node.value;
          return n == null || allowScalar && identity.isScalar(n) && n.value == null && !n.commentBefore && !n.comment && !n.tag;
        });
      }
      /**
       * Checks if the collection includes a value with the key `key`.
       */
      hasIn(path) {
        const [key, ...rest] = path;
        if (rest.length === 0)
          return this.has(key);
        const node = this.get(key, true);
        return identity.isCollection(node) ? node.hasIn(rest) : false;
      }
      /**
       * Sets a value in this collection. For `!!set`, `value` needs to be a
       * boolean to add/remove the item from the set.
       */
      setIn(path, value) {
        const [key, ...rest] = path;
        if (rest.length === 0) {
          this.set(key, value);
        } else {
          const node = this.get(key, true);
          if (identity.isCollection(node))
            node.setIn(rest, value);
          else if (node === void 0 && this.schema)
            this.set(key, collectionFromPath(this.schema, rest, value));
          else
            throw new Error(`Expected YAML collection at ${key}. Remaining path: ${rest}`);
        }
      }
    };
    exports.Collection = Collection;
    exports.collectionFromPath = collectionFromPath;
    exports.isEmptyPath = isEmptyPath;
  }
});

// node_modules/yaml/dist/stringify/stringifyComment.js
var require_stringifyComment = __commonJS({
  "node_modules/yaml/dist/stringify/stringifyComment.js"(exports) {
    "use strict";
    var stringifyComment = (str) => str.replace(/^(?!$)(?: $)?/gm, "#");
    function indentComment(comment, indent) {
      if (/^\n+$/.test(comment))
        return comment.substring(1);
      return indent ? comment.replace(/^(?! *$)/gm, indent) : comment;
    }
    var lineComment = (str, indent, comment) => str.endsWith("\n") ? indentComment(comment, indent) : comment.includes("\n") ? "\n" + indentComment(comment, indent) : (str.endsWith(" ") ? "" : " ") + comment;
    exports.indentComment = indentComment;
    exports.lineComment = lineComment;
    exports.stringifyComment = stringifyComment;
  }
});

// node_modules/yaml/dist/stringify/foldFlowLines.js
var require_foldFlowLines = __commonJS({
  "node_modules/yaml/dist/stringify/foldFlowLines.js"(exports) {
    "use strict";
    var FOLD_FLOW = "flow";
    var FOLD_BLOCK = "block";
    var FOLD_QUOTED = "quoted";
    function foldFlowLines(text, indent, mode = "flow", { indentAtStart, lineWidth = 80, minContentWidth = 20, onFold, onOverflow } = {}) {
      if (!lineWidth || lineWidth < 0)
        return text;
      if (lineWidth < minContentWidth)
        minContentWidth = 0;
      const endStep = Math.max(1 + minContentWidth, 1 + lineWidth - indent.length);
      if (text.length <= endStep)
        return text;
      const folds = [];
      const escapedFolds = {};
      let end = lineWidth - indent.length;
      if (typeof indentAtStart === "number") {
        if (indentAtStart > lineWidth - Math.max(2, minContentWidth))
          folds.push(0);
        else
          end = lineWidth - indentAtStart;
      }
      let split = void 0;
      let prev = void 0;
      let overflow = false;
      let i = -1;
      let escStart = -1;
      let escEnd = -1;
      if (mode === FOLD_BLOCK) {
        i = consumeMoreIndentedLines(text, i, indent.length);
        if (i !== -1)
          end = i + endStep;
      }
      for (let ch; ch = text[i += 1]; ) {
        if (mode === FOLD_QUOTED && ch === "\\") {
          escStart = i;
          switch (text[i + 1]) {
            case "x":
              i += 3;
              break;
            case "u":
              i += 5;
              break;
            case "U":
              i += 9;
              break;
            default:
              i += 1;
          }
          escEnd = i;
        }
        if (ch === "\n") {
          if (mode === FOLD_BLOCK)
            i = consumeMoreIndentedLines(text, i, indent.length);
          end = i + indent.length + endStep;
          split = void 0;
        } else {
          if (ch === " " && prev && prev !== " " && prev !== "\n" && prev !== "	") {
            const next = text[i + 1];
            if (next && next !== " " && next !== "\n" && next !== "	")
              split = i;
          }
          if (i >= end) {
            if (split) {
              folds.push(split);
              end = split + endStep;
              split = void 0;
            } else if (mode === FOLD_QUOTED) {
              while (prev === " " || prev === "	") {
                prev = ch;
                ch = text[i += 1];
                overflow = true;
              }
              const j = i > escEnd + 1 ? i - 2 : escStart - 1;
              if (escapedFolds[j])
                return text;
              folds.push(j);
              escapedFolds[j] = true;
              end = j + endStep;
              split = void 0;
            } else {
              overflow = true;
            }
          }
        }
        prev = ch;
      }
      if (overflow && onOverflow)
        onOverflow();
      if (folds.length === 0)
        return text;
      if (onFold)
        onFold();
      let res = text.slice(0, folds[0]);
      for (let i2 = 0; i2 < folds.length; ++i2) {
        const fold = folds[i2];
        const end2 = folds[i2 + 1] || text.length;
        if (fold === 0)
          res = `
${indent}${text.slice(0, end2)}`;
        else {
          if (mode === FOLD_QUOTED && escapedFolds[fold])
            res += `${text[fold]}\\`;
          res += `
${indent}${text.slice(fold + 1, end2)}`;
        }
      }
      return res;
    }
    function consumeMoreIndentedLines(text, i, indent) {
      let end = i;
      let start = i + 1;
      let ch = text[start];
      while (ch === " " || ch === "	") {
        if (i < start + indent) {
          ch = text[++i];
        } else {
          do {
            ch = text[++i];
          } while (ch && ch !== "\n");
          end = i;
          start = i + 1;
          ch = text[start];
        }
      }
      return end;
    }
    exports.FOLD_BLOCK = FOLD_BLOCK;
    exports.FOLD_FLOW = FOLD_FLOW;
    exports.FOLD_QUOTED = FOLD_QUOTED;
    exports.foldFlowLines = foldFlowLines;
  }
});

// node_modules/yaml/dist/stringify/stringifyString.js
var require_stringifyString = __commonJS({
  "node_modules/yaml/dist/stringify/stringifyString.js"(exports) {
    "use strict";
    var Scalar = require_Scalar();
    var foldFlowLines = require_foldFlowLines();
    var getFoldOptions = (ctx, isBlock) => ({
      indentAtStart: isBlock ? ctx.indent.length : ctx.indentAtStart,
      lineWidth: ctx.options.lineWidth,
      minContentWidth: ctx.options.minContentWidth
    });
    var containsDocumentMarker = (str) => /^(%|---|\.\.\.)/m.test(str);
    function lineLengthOverLimit(str, lineWidth, indentLength) {
      if (!lineWidth || lineWidth < 0)
        return false;
      const limit = lineWidth - indentLength;
      const strLen = str.length;
      if (strLen <= limit)
        return false;
      for (let i = 0, start = 0; i < strLen; ++i) {
        if (str[i] === "\n") {
          if (i - start > limit)
            return true;
          start = i + 1;
          if (strLen - start <= limit)
            return false;
        }
      }
      return true;
    }
    function doubleQuotedString(value, ctx) {
      const json = JSON.stringify(value);
      if (ctx.options.doubleQuotedAsJSON)
        return json;
      const { implicitKey } = ctx;
      const minMultiLineLength = ctx.options.doubleQuotedMinMultiLineLength;
      const indent = ctx.indent || (containsDocumentMarker(value) ? "  " : "");
      let str = "";
      let start = 0;
      for (let i = 0, ch = json[i]; ch; ch = json[++i]) {
        if (ch === " " && json[i + 1] === "\\" && json[i + 2] === "n") {
          str += json.slice(start, i) + "\\ ";
          i += 1;
          start = i;
          ch = "\\";
        }
        if (ch === "\\")
          switch (json[i + 1]) {
            case "u":
              {
                str += json.slice(start, i);
                const code = json.substr(i + 2, 4);
                switch (code) {
                  case "0000":
                    str += "\\0";
                    break;
                  case "0007":
                    str += "\\a";
                    break;
                  case "000b":
                    str += "\\v";
                    break;
                  case "001b":
                    str += "\\e";
                    break;
                  case "0085":
                    str += "\\N";
                    break;
                  case "00a0":
                    str += "\\_";
                    break;
                  case "2028":
                    str += "\\L";
                    break;
                  case "2029":
                    str += "\\P";
                    break;
                  default:
                    if (code.substr(0, 2) === "00")
                      str += "\\x" + code.substr(2);
                    else
                      str += json.substr(i, 6);
                }
                i += 5;
                start = i + 1;
              }
              break;
            case "n":
              if (implicitKey || json[i + 2] === '"' || json.length < minMultiLineLength) {
                i += 1;
              } else {
                str += json.slice(start, i) + "\n\n";
                while (json[i + 2] === "\\" && json[i + 3] === "n" && json[i + 4] !== '"') {
                  str += "\n";
                  i += 2;
                }
                str += indent;
                if (json[i + 2] === " ")
                  str += "\\";
                i += 1;
                start = i + 1;
              }
              break;
            default:
              i += 1;
          }
      }
      str = start ? str + json.slice(start) : json;
      return implicitKey ? str : foldFlowLines.foldFlowLines(str, indent, foldFlowLines.FOLD_QUOTED, getFoldOptions(ctx, false));
    }
    function singleQuotedString(value, ctx) {
      if (ctx.options.singleQuote === false || ctx.implicitKey && value.includes("\n") || /[ \t]\n|\n[ \t]/.test(value))
        return doubleQuotedString(value, ctx);
      const indent = ctx.indent || (containsDocumentMarker(value) ? "  " : "");
      const res = "'" + value.replace(/'/g, "''").replace(/\n+/g, `$&
${indent}`) + "'";
      return ctx.implicitKey ? res : foldFlowLines.foldFlowLines(res, indent, foldFlowLines.FOLD_FLOW, getFoldOptions(ctx, false));
    }
    function quotedString(value, ctx) {
      const { singleQuote } = ctx.options;
      let qs;
      if (singleQuote === false)
        qs = doubleQuotedString;
      else {
        const hasDouble = value.includes('"');
        const hasSingle = value.includes("'");
        if (hasDouble && !hasSingle)
          qs = singleQuotedString;
        else if (hasSingle && !hasDouble)
          qs = doubleQuotedString;
        else
          qs = singleQuote ? singleQuotedString : doubleQuotedString;
      }
      return qs(value, ctx);
    }
    var blockEndNewlines;
    try {
      blockEndNewlines = new RegExp("(^|(?<!\n))\n+(?!\n|$)", "g");
    } catch {
      blockEndNewlines = /\n+(?!\n|$)/g;
    }
    function blockString({ comment, type, value }, ctx, onComment, onChompKeep) {
      const { blockQuote, commentString, lineWidth } = ctx.options;
      if (!blockQuote || /\n[\t ]+$/.test(value)) {
        return quotedString(value, ctx);
      }
      const indent = ctx.indent || (ctx.forceBlockIndent || containsDocumentMarker(value) ? "  " : "");
      const literal = blockQuote === "literal" ? true : blockQuote === "folded" || type === Scalar.Scalar.BLOCK_FOLDED ? false : type === Scalar.Scalar.BLOCK_LITERAL ? true : !lineLengthOverLimit(value, lineWidth, indent.length);
      if (!value)
        return literal ? "|\n" : ">\n";
      let chomp;
      let endStart;
      for (endStart = value.length; endStart > 0; --endStart) {
        const ch = value[endStart - 1];
        if (ch !== "\n" && ch !== "	" && ch !== " ")
          break;
      }
      let end = value.substring(endStart);
      const endNlPos = end.indexOf("\n");
      if (endNlPos === -1) {
        chomp = "-";
      } else if (value === end || endNlPos !== end.length - 1) {
        chomp = "+";
        if (onChompKeep)
          onChompKeep();
      } else {
        chomp = "";
      }
      if (end) {
        value = value.slice(0, -end.length);
        if (end[end.length - 1] === "\n")
          end = end.slice(0, -1);
        end = end.replace(blockEndNewlines, `$&${indent}`);
      }
      let startWithSpace = false;
      let startEnd;
      let startNlPos = -1;
      for (startEnd = 0; startEnd < value.length; ++startEnd) {
        const ch = value[startEnd];
        if (ch === " ")
          startWithSpace = true;
        else if (ch === "\n")
          startNlPos = startEnd;
        else
          break;
      }
      let start = value.substring(0, startNlPos < startEnd ? startNlPos + 1 : startEnd);
      if (start) {
        value = value.substring(start.length);
        start = start.replace(/\n+/g, `$&${indent}`);
      }
      const indentSize = indent ? "2" : "1";
      let header = (startWithSpace ? indentSize : "") + chomp;
      if (comment) {
        header += " " + commentString(comment.replace(/ ?[\r\n]+/g, " "));
        if (onComment)
          onComment();
      }
      if (!literal) {
        const foldedValue = value.replace(/\n+/g, "\n$&").replace(/(?:^|\n)([\t ].*)(?:([\n\t ]*)\n(?![\n\t ]))?/g, "$1$2").replace(/\n+/g, `$&${indent}`);
        let literalFallback = false;
        const foldOptions = getFoldOptions(ctx, true);
        if (blockQuote !== "folded" && type !== Scalar.Scalar.BLOCK_FOLDED) {
          foldOptions.onOverflow = () => {
            literalFallback = true;
          };
        }
        const body = foldFlowLines.foldFlowLines(`${start}${foldedValue}${end}`, indent, foldFlowLines.FOLD_BLOCK, foldOptions);
        if (!literalFallback)
          return `>${header}
${indent}${body}`;
      }
      value = value.replace(/\n+/g, `$&${indent}`);
      return `|${header}
${indent}${start}${value}${end}`;
    }
    function plainString(item, ctx, onComment, onChompKeep) {
      const { type, value } = item;
      const { actualString, implicitKey, indent, indentStep, inFlow } = ctx;
      if (implicitKey && value.includes("\n") || inFlow && /[[\]{},]/.test(value)) {
        return quotedString(value, ctx);
      }
      if (/^[\n\t ,[\]{}#&*!|>'"%@`]|^[?-]$|^[?-][ \t]|[\n:][ \t]|[ \t]\n|[\n\t ]#|[\n\t :]$/.test(value)) {
        return implicitKey || inFlow || !value.includes("\n") ? quotedString(value, ctx) : blockString(item, ctx, onComment, onChompKeep);
      }
      if (!implicitKey && !inFlow && type !== Scalar.Scalar.PLAIN && value.includes("\n")) {
        return blockString(item, ctx, onComment, onChompKeep);
      }
      if (containsDocumentMarker(value)) {
        if (indent === "") {
          ctx.forceBlockIndent = true;
          return blockString(item, ctx, onComment, onChompKeep);
        } else if (implicitKey && indent === indentStep) {
          return quotedString(value, ctx);
        }
      }
      const str = value.replace(/\n+/g, `$&
${indent}`);
      if (actualString) {
        const test = (tag) => tag.default && tag.tag !== "tag:yaml.org,2002:str" && tag.test?.test(str);
        const { compat, tags } = ctx.doc.schema;
        if (tags.some(test) || compat?.some(test))
          return quotedString(value, ctx);
      }
      return implicitKey ? str : foldFlowLines.foldFlowLines(str, indent, foldFlowLines.FOLD_FLOW, getFoldOptions(ctx, false));
    }
    function stringifyString(item, ctx, onComment, onChompKeep) {
      const { implicitKey, inFlow } = ctx;
      const ss = typeof item.value === "string" ? item : Object.assign({}, item, { value: String(item.value) });
      let { type } = item;
      if (type !== Scalar.Scalar.QUOTE_DOUBLE) {
        if (/[\x00-\x08\x0b-\x1f\x7f-\x9f\u{D800}-\u{DFFF}]/u.test(ss.value))
          type = Scalar.Scalar.QUOTE_DOUBLE;
      }
      const _stringify = (_type) => {
        switch (_type) {
          case Scalar.Scalar.BLOCK_FOLDED:
          case Scalar.Scalar.BLOCK_LITERAL:
            return implicitKey || inFlow ? quotedString(ss.value, ctx) : blockString(ss, ctx, onComment, onChompKeep);
          case Scalar.Scalar.QUOTE_DOUBLE:
            return doubleQuotedString(ss.value, ctx);
          case Scalar.Scalar.QUOTE_SINGLE:
            return singleQuotedString(ss.value, ctx);
          case Scalar.Scalar.PLAIN:
            return plainString(ss, ctx, onComment, onChompKeep);
          default:
            return null;
        }
      };
      let res = _stringify(type);
      if (res === null) {
        const { defaultKeyType, defaultStringType } = ctx.options;
        const t = implicitKey && defaultKeyType || defaultStringType;
        res = _stringify(t);
        if (res === null)
          throw new Error(`Unsupported default string type ${t}`);
      }
      return res;
    }
    exports.stringifyString = stringifyString;
  }
});

// node_modules/yaml/dist/stringify/stringify.js
var require_stringify = __commonJS({
  "node_modules/yaml/dist/stringify/stringify.js"(exports) {
    "use strict";
    var anchors = require_anchors();
    var identity = require_identity();
    var stringifyComment = require_stringifyComment();
    var stringifyString = require_stringifyString();
    function createStringifyContext(doc, options) {
      const opt = Object.assign({
        blockQuote: true,
        commentString: stringifyComment.stringifyComment,
        defaultKeyType: null,
        defaultStringType: "PLAIN",
        directives: null,
        doubleQuotedAsJSON: false,
        doubleQuotedMinMultiLineLength: 40,
        falseStr: "false",
        flowCollectionPadding: true,
        indentSeq: true,
        lineWidth: 80,
        minContentWidth: 20,
        nullStr: "null",
        simpleKeys: false,
        singleQuote: null,
        trailingComma: false,
        trueStr: "true",
        verifyAliasOrder: true
      }, doc.schema.toStringOptions, options);
      let inFlow;
      switch (opt.collectionStyle) {
        case "block":
          inFlow = false;
          break;
        case "flow":
          inFlow = true;
          break;
        default:
          inFlow = null;
      }
      return {
        anchors: /* @__PURE__ */ new Set(),
        doc,
        flowCollectionPadding: opt.flowCollectionPadding ? " " : "",
        indent: "",
        indentStep: typeof opt.indent === "number" ? " ".repeat(opt.indent) : "  ",
        inFlow,
        options: opt
      };
    }
    function getTagObject(tags, item) {
      if (item.tag) {
        const match = tags.filter((t) => t.tag === item.tag);
        if (match.length > 0)
          return match.find((t) => t.format === item.format) ?? match[0];
      }
      let tagObj = void 0;
      let obj;
      if (identity.isScalar(item)) {
        obj = item.value;
        let match = tags.filter((t) => t.identify?.(obj));
        if (match.length > 1) {
          const testMatch = match.filter((t) => t.test);
          if (testMatch.length > 0)
            match = testMatch;
        }
        tagObj = match.find((t) => t.format === item.format) ?? match.find((t) => !t.format);
      } else {
        obj = item;
        tagObj = tags.find((t) => t.nodeClass && obj instanceof t.nodeClass);
      }
      if (!tagObj) {
        const name = obj?.constructor?.name ?? (obj === null ? "null" : typeof obj);
        throw new Error(`Tag not resolved for ${name} value`);
      }
      return tagObj;
    }
    function stringifyProps(node, tagObj, { anchors: anchors$1, doc }) {
      if (!doc.directives)
        return "";
      const props = [];
      const anchor = (identity.isScalar(node) || identity.isCollection(node)) && node.anchor;
      if (anchor && anchors.anchorIsValid(anchor)) {
        anchors$1.add(anchor);
        props.push(`&${anchor}`);
      }
      const tag = node.tag ?? (tagObj.default ? null : tagObj.tag);
      if (tag)
        props.push(doc.directives.tagString(tag));
      return props.join(" ");
    }
    function stringify2(item, ctx, onComment, onChompKeep) {
      if (identity.isPair(item))
        return item.toString(ctx, onComment, onChompKeep);
      if (identity.isAlias(item)) {
        if (ctx.doc.directives)
          return item.toString(ctx);
        if (ctx.resolvedAliases?.has(item)) {
          throw new TypeError(`Cannot stringify circular structure without alias nodes`);
        } else {
          if (ctx.resolvedAliases)
            ctx.resolvedAliases.add(item);
          else
            ctx.resolvedAliases = /* @__PURE__ */ new Set([item]);
          item = item.resolve(ctx.doc);
        }
      }
      let tagObj = void 0;
      const node = identity.isNode(item) ? item : ctx.doc.createNode(item, { onTagObj: (o) => tagObj = o });
      tagObj ?? (tagObj = getTagObject(ctx.doc.schema.tags, node));
      const props = stringifyProps(node, tagObj, ctx);
      if (props.length > 0)
        ctx.indentAtStart = (ctx.indentAtStart ?? 0) + props.length + 1;
      const str = typeof tagObj.stringify === "function" ? tagObj.stringify(node, ctx, onComment, onChompKeep) : identity.isScalar(node) ? stringifyString.stringifyString(node, ctx, onComment, onChompKeep) : node.toString(ctx, onComment, onChompKeep);
      if (!props)
        return str;
      return identity.isScalar(node) || str[0] === "{" || str[0] === "[" ? `${props} ${str}` : `${props}
${ctx.indent}${str}`;
    }
    exports.createStringifyContext = createStringifyContext;
    exports.stringify = stringify2;
  }
});

// node_modules/yaml/dist/stringify/stringifyPair.js
var require_stringifyPair = __commonJS({
  "node_modules/yaml/dist/stringify/stringifyPair.js"(exports) {
    "use strict";
    var identity = require_identity();
    var Scalar = require_Scalar();
    var stringify2 = require_stringify();
    var stringifyComment = require_stringifyComment();
    function stringifyPair({ key, value }, ctx, onComment, onChompKeep) {
      const { allNullValues, doc, indent, indentStep, options: { commentString, indentSeq, simpleKeys } } = ctx;
      let keyComment = identity.isNode(key) && key.comment || null;
      if (simpleKeys) {
        if (keyComment) {
          throw new Error("With simple keys, key nodes cannot have comments");
        }
        if (identity.isCollection(key) || !identity.isNode(key) && typeof key === "object") {
          const msg = "With simple keys, collection cannot be used as a key value";
          throw new Error(msg);
        }
      }
      let explicitKey = !simpleKeys && (!key || keyComment && value == null && !ctx.inFlow || identity.isCollection(key) || (identity.isScalar(key) ? key.type === Scalar.Scalar.BLOCK_FOLDED || key.type === Scalar.Scalar.BLOCK_LITERAL : typeof key === "object"));
      ctx = Object.assign({}, ctx, {
        allNullValues: false,
        implicitKey: !explicitKey && (simpleKeys || !allNullValues),
        indent: indent + indentStep
      });
      let keyCommentDone = false;
      let chompKeep = false;
      let str = stringify2.stringify(key, ctx, () => keyCommentDone = true, () => chompKeep = true);
      if (!explicitKey && !ctx.inFlow && str.length > 1024) {
        if (simpleKeys)
          throw new Error("With simple keys, single line scalar must not span more than 1024 characters");
        explicitKey = true;
      }
      if (ctx.inFlow) {
        if (allNullValues || value == null) {
          if (keyCommentDone && onComment)
            onComment();
          return str === "" ? "?" : explicitKey ? `? ${str}` : str;
        }
      } else if (allNullValues && !simpleKeys || value == null && explicitKey) {
        str = `? ${str}`;
        if (keyComment && !keyCommentDone) {
          str += stringifyComment.lineComment(str, ctx.indent, commentString(keyComment));
        } else if (chompKeep && onChompKeep)
          onChompKeep();
        return str;
      }
      if (keyCommentDone)
        keyComment = null;
      if (explicitKey) {
        if (keyComment)
          str += stringifyComment.lineComment(str, ctx.indent, commentString(keyComment));
        str = `? ${str}
${indent}:`;
      } else {
        str = `${str}:`;
        if (keyComment)
          str += stringifyComment.lineComment(str, ctx.indent, commentString(keyComment));
      }
      let vsb, vcb, valueComment;
      if (identity.isNode(value)) {
        vsb = !!value.spaceBefore;
        vcb = value.commentBefore;
        valueComment = value.comment;
      } else {
        vsb = false;
        vcb = null;
        valueComment = null;
        if (value && typeof value === "object")
          value = doc.createNode(value);
      }
      ctx.implicitKey = false;
      if (!explicitKey && !keyComment && identity.isScalar(value))
        ctx.indentAtStart = str.length + 1;
      chompKeep = false;
      if (!indentSeq && indentStep.length >= 2 && !ctx.inFlow && !explicitKey && identity.isSeq(value) && !value.flow && !value.tag && !value.anchor) {
        ctx.indent = ctx.indent.substring(2);
      }
      let valueCommentDone = false;
      const valueStr = stringify2.stringify(value, ctx, () => valueCommentDone = true, () => chompKeep = true);
      let ws = " ";
      if (keyComment || vsb || vcb) {
        ws = vsb ? "\n" : "";
        if (vcb) {
          const cs = commentString(vcb);
          ws += `
${stringifyComment.indentComment(cs, ctx.indent)}`;
        }
        if (valueStr === "" && !ctx.inFlow) {
          if (ws === "\n" && valueComment)
            ws = "\n\n";
        } else {
          ws += `
${ctx.indent}`;
        }
      } else if (!explicitKey && identity.isCollection(value)) {
        const vs0 = valueStr[0];
        const nl0 = valueStr.indexOf("\n");
        const hasNewline = nl0 !== -1;
        const flow = ctx.inFlow ?? value.flow ?? value.items.length === 0;
        if (hasNewline || !flow) {
          let hasPropsLine = false;
          if (hasNewline && (vs0 === "&" || vs0 === "!")) {
            let sp0 = valueStr.indexOf(" ");
            if (vs0 === "&" && sp0 !== -1 && sp0 < nl0 && valueStr[sp0 + 1] === "!") {
              sp0 = valueStr.indexOf(" ", sp0 + 1);
            }
            if (sp0 === -1 || nl0 < sp0)
              hasPropsLine = true;
          }
          if (!hasPropsLine)
            ws = `
${ctx.indent}`;
        }
      } else if (valueStr === "" || valueStr[0] === "\n") {
        ws = "";
      }
      str += ws + valueStr;
      if (ctx.inFlow) {
        if (valueCommentDone && onComment)
          onComment();
      } else if (valueComment && !valueCommentDone) {
        str += stringifyComment.lineComment(str, ctx.indent, commentString(valueComment));
      } else if (chompKeep && onChompKeep) {
        onChompKeep();
      }
      return str;
    }
    exports.stringifyPair = stringifyPair;
  }
});

// node_modules/yaml/dist/log.js
var require_log = __commonJS({
  "node_modules/yaml/dist/log.js"(exports) {
    "use strict";
    var node_process = __require("process");
    function debug(logLevel, ...messages) {
      if (logLevel === "debug")
        console.log(...messages);
    }
    function warn(logLevel, warning) {
      if (logLevel === "debug" || logLevel === "warn") {
        if (typeof node_process.emitWarning === "function")
          node_process.emitWarning(warning);
        else
          console.warn(warning);
      }
    }
    exports.debug = debug;
    exports.warn = warn;
  }
});

// node_modules/yaml/dist/schema/yaml-1.1/merge.js
var require_merge = __commonJS({
  "node_modules/yaml/dist/schema/yaml-1.1/merge.js"(exports) {
    "use strict";
    var identity = require_identity();
    var Scalar = require_Scalar();
    var MERGE_KEY = "<<";
    var merge = {
      identify: (value) => value === MERGE_KEY || typeof value === "symbol" && value.description === MERGE_KEY,
      default: "key",
      tag: "tag:yaml.org,2002:merge",
      test: /^<<$/,
      resolve: () => Object.assign(new Scalar.Scalar(Symbol(MERGE_KEY)), {
        addToJSMap: addMergeToJSMap
      }),
      stringify: () => MERGE_KEY
    };
    var isMergeKey = (ctx, key) => (merge.identify(key) || identity.isScalar(key) && (!key.type || key.type === Scalar.Scalar.PLAIN) && merge.identify(key.value)) && ctx?.doc.schema.tags.some((tag) => tag.tag === merge.tag && tag.default);
    function addMergeToJSMap(ctx, map, value) {
      const source = resolveAliasValue(ctx, value);
      if (identity.isSeq(source))
        for (const it of source.items)
          mergeValue(ctx, map, it);
      else if (Array.isArray(source))
        for (const it of source)
          mergeValue(ctx, map, it);
      else
        mergeValue(ctx, map, source);
    }
    function mergeValue(ctx, map, value) {
      const source = resolveAliasValue(ctx, value);
      if (!identity.isMap(source))
        throw new Error("Merge sources must be maps or map aliases");
      const srcMap = source.toJSON(null, ctx, Map);
      for (const [key, value2] of srcMap) {
        if (map instanceof Map) {
          if (!map.has(key))
            map.set(key, value2);
        } else if (map instanceof Set) {
          map.add(key);
        } else if (!Object.prototype.hasOwnProperty.call(map, key)) {
          Object.defineProperty(map, key, {
            value: value2,
            writable: true,
            enumerable: true,
            configurable: true
          });
        }
      }
      return map;
    }
    function resolveAliasValue(ctx, value) {
      return ctx && identity.isAlias(value) ? value.resolve(ctx.doc, ctx) : value;
    }
    exports.addMergeToJSMap = addMergeToJSMap;
    exports.isMergeKey = isMergeKey;
    exports.merge = merge;
  }
});

// node_modules/yaml/dist/nodes/addPairToJSMap.js
var require_addPairToJSMap = __commonJS({
  "node_modules/yaml/dist/nodes/addPairToJSMap.js"(exports) {
    "use strict";
    var log = require_log();
    var merge = require_merge();
    var stringify2 = require_stringify();
    var identity = require_identity();
    var toJS = require_toJS();
    function addPairToJSMap(ctx, map, { key, value }) {
      if (identity.isNode(key) && key.addToJSMap)
        key.addToJSMap(ctx, map, value);
      else if (merge.isMergeKey(ctx, key))
        merge.addMergeToJSMap(ctx, map, value);
      else {
        const jsKey = toJS.toJS(key, "", ctx);
        if (map instanceof Map) {
          map.set(jsKey, toJS.toJS(value, jsKey, ctx));
        } else if (map instanceof Set) {
          map.add(jsKey);
        } else {
          const stringKey = stringifyKey(key, jsKey, ctx);
          const jsValue = toJS.toJS(value, stringKey, ctx);
          if (stringKey in map)
            Object.defineProperty(map, stringKey, {
              value: jsValue,
              writable: true,
              enumerable: true,
              configurable: true
            });
          else
            map[stringKey] = jsValue;
        }
      }
      return map;
    }
    function stringifyKey(key, jsKey, ctx) {
      if (jsKey === null)
        return "";
      if (typeof jsKey !== "object")
        return String(jsKey);
      if (identity.isNode(key) && ctx?.doc) {
        const strCtx = stringify2.createStringifyContext(ctx.doc, {});
        strCtx.anchors = /* @__PURE__ */ new Set();
        for (const node of ctx.anchors.keys())
          strCtx.anchors.add(node.anchor);
        strCtx.inFlow = true;
        strCtx.inStringifyKey = true;
        const strKey = key.toString(strCtx);
        if (!ctx.mapKeyWarned) {
          let jsonStr = JSON.stringify(strKey);
          if (jsonStr.length > 40)
            jsonStr = jsonStr.substring(0, 36) + '..."';
          log.warn(ctx.doc.options.logLevel, `Keys with collection values will be stringified due to JS Object restrictions: ${jsonStr}. Set mapAsMap: true to use object keys.`);
          ctx.mapKeyWarned = true;
        }
        return strKey;
      }
      return JSON.stringify(jsKey);
    }
    exports.addPairToJSMap = addPairToJSMap;
  }
});

// node_modules/yaml/dist/nodes/Pair.js
var require_Pair = __commonJS({
  "node_modules/yaml/dist/nodes/Pair.js"(exports) {
    "use strict";
    var createNode = require_createNode();
    var stringifyPair = require_stringifyPair();
    var addPairToJSMap = require_addPairToJSMap();
    var identity = require_identity();
    function createPair(key, value, ctx) {
      const k = createNode.createNode(key, void 0, ctx);
      const v = createNode.createNode(value, void 0, ctx);
      return new Pair(k, v);
    }
    var Pair = class _Pair {
      constructor(key, value = null) {
        Object.defineProperty(this, identity.NODE_TYPE, { value: identity.PAIR });
        this.key = key;
        this.value = value;
      }
      clone(schema) {
        let { key, value } = this;
        if (identity.isNode(key))
          key = key.clone(schema);
        if (identity.isNode(value))
          value = value.clone(schema);
        return new _Pair(key, value);
      }
      toJSON(_, ctx) {
        const pair = ctx?.mapAsMap ? /* @__PURE__ */ new Map() : {};
        return addPairToJSMap.addPairToJSMap(ctx, pair, this);
      }
      toString(ctx, onComment, onChompKeep) {
        return ctx?.doc ? stringifyPair.stringifyPair(this, ctx, onComment, onChompKeep) : JSON.stringify(this);
      }
    };
    exports.Pair = Pair;
    exports.createPair = createPair;
  }
});

// node_modules/yaml/dist/stringify/stringifyCollection.js
var require_stringifyCollection = __commonJS({
  "node_modules/yaml/dist/stringify/stringifyCollection.js"(exports) {
    "use strict";
    var identity = require_identity();
    var stringify2 = require_stringify();
    var stringifyComment = require_stringifyComment();
    function stringifyCollection(collection, ctx, options) {
      const flow = ctx.inFlow ?? collection.flow;
      const stringify3 = flow ? stringifyFlowCollection : stringifyBlockCollection;
      return stringify3(collection, ctx, options);
    }
    function stringifyBlockCollection({ comment, items }, ctx, { blockItemPrefix, flowChars, itemIndent, onChompKeep, onComment }) {
      const { indent, options: { commentString } } = ctx;
      const itemCtx = Object.assign({}, ctx, { indent: itemIndent, type: null });
      let chompKeep = false;
      const lines = [];
      for (let i = 0; i < items.length; ++i) {
        const item = items[i];
        let comment2 = null;
        if (identity.isNode(item)) {
          if (!chompKeep && item.spaceBefore)
            lines.push("");
          addCommentBefore(ctx, lines, item.commentBefore, chompKeep);
          if (item.comment)
            comment2 = item.comment;
        } else if (identity.isPair(item)) {
          const ik = identity.isNode(item.key) ? item.key : null;
          if (ik) {
            if (!chompKeep && ik.spaceBefore)
              lines.push("");
            addCommentBefore(ctx, lines, ik.commentBefore, chompKeep);
          }
        }
        chompKeep = false;
        let str2 = stringify2.stringify(item, itemCtx, () => comment2 = null, () => chompKeep = true);
        if (comment2)
          str2 += stringifyComment.lineComment(str2, itemIndent, commentString(comment2));
        if (chompKeep && comment2)
          chompKeep = false;
        lines.push(blockItemPrefix + str2);
      }
      let str;
      if (lines.length === 0) {
        str = flowChars.start + flowChars.end;
      } else {
        str = lines[0];
        for (let i = 1; i < lines.length; ++i) {
          const line = lines[i];
          str += line ? `
${indent}${line}` : "\n";
        }
      }
      if (comment) {
        str += "\n" + stringifyComment.indentComment(commentString(comment), indent);
        if (onComment)
          onComment();
      } else if (chompKeep && onChompKeep)
        onChompKeep();
      return str;
    }
    function stringifyFlowCollection({ items }, ctx, { flowChars, itemIndent }) {
      const { indent, indentStep, flowCollectionPadding: fcPadding, options: { commentString } } = ctx;
      itemIndent += indentStep;
      const itemCtx = Object.assign({}, ctx, {
        indent: itemIndent,
        inFlow: true,
        type: null
      });
      let reqNewline = false;
      let linesAtValue = 0;
      const lines = [];
      for (let i = 0; i < items.length; ++i) {
        const item = items[i];
        let comment = null;
        if (identity.isNode(item)) {
          if (item.spaceBefore)
            lines.push("");
          addCommentBefore(ctx, lines, item.commentBefore, false);
          if (item.comment)
            comment = item.comment;
        } else if (identity.isPair(item)) {
          const ik = identity.isNode(item.key) ? item.key : null;
          if (ik) {
            if (ik.spaceBefore)
              lines.push("");
            addCommentBefore(ctx, lines, ik.commentBefore, false);
            if (ik.comment)
              reqNewline = true;
          }
          const iv = identity.isNode(item.value) ? item.value : null;
          if (iv) {
            if (iv.comment)
              comment = iv.comment;
            if (iv.commentBefore)
              reqNewline = true;
          } else if (item.value == null && ik?.comment) {
            comment = ik.comment;
          }
        }
        if (comment)
          reqNewline = true;
        let str = stringify2.stringify(item, itemCtx, () => comment = null);
        reqNewline || (reqNewline = lines.length > linesAtValue || str.includes("\n"));
        if (i < items.length - 1) {
          str += ",";
        } else if (ctx.options.trailingComma) {
          if (ctx.options.lineWidth > 0) {
            reqNewline || (reqNewline = lines.reduce((sum, line) => sum + line.length + 2, 2) + (str.length + 2) > ctx.options.lineWidth);
          }
          if (reqNewline) {
            str += ",";
          }
        }
        if (comment)
          str += stringifyComment.lineComment(str, itemIndent, commentString(comment));
        lines.push(str);
        linesAtValue = lines.length;
      }
      const { start, end } = flowChars;
      if (lines.length === 0) {
        return start + end;
      } else {
        if (!reqNewline) {
          const len = lines.reduce((sum, line) => sum + line.length + 2, 2);
          reqNewline = ctx.options.lineWidth > 0 && len > ctx.options.lineWidth;
        }
        if (reqNewline) {
          let str = start;
          for (const line of lines)
            str += line ? `
${indentStep}${indent}${line}` : "\n";
          return `${str}
${indent}${end}`;
        } else {
          return `${start}${fcPadding}${lines.join(" ")}${fcPadding}${end}`;
        }
      }
    }
    function addCommentBefore({ indent, options: { commentString } }, lines, comment, chompKeep) {
      if (comment && chompKeep)
        comment = comment.replace(/^\n+/, "");
      if (comment) {
        const ic = stringifyComment.indentComment(commentString(comment), indent);
        lines.push(ic.trimStart());
      }
    }
    exports.stringifyCollection = stringifyCollection;
  }
});

// node_modules/yaml/dist/nodes/YAMLMap.js
var require_YAMLMap = __commonJS({
  "node_modules/yaml/dist/nodes/YAMLMap.js"(exports) {
    "use strict";
    var stringifyCollection = require_stringifyCollection();
    var addPairToJSMap = require_addPairToJSMap();
    var Collection = require_Collection();
    var identity = require_identity();
    var Pair = require_Pair();
    var Scalar = require_Scalar();
    function findPair(items, key) {
      const k = identity.isScalar(key) ? key.value : key;
      for (const it of items) {
        if (identity.isPair(it)) {
          if (it.key === key || it.key === k)
            return it;
          if (identity.isScalar(it.key) && it.key.value === k)
            return it;
        }
      }
      return void 0;
    }
    var YAMLMap = class extends Collection.Collection {
      static get tagName() {
        return "tag:yaml.org,2002:map";
      }
      constructor(schema) {
        super(identity.MAP, schema);
        this.items = [];
      }
      /**
       * A generic collection parsing method that can be extended
       * to other node classes that inherit from YAMLMap
       */
      static from(schema, obj, ctx) {
        const { keepUndefined, replacer } = ctx;
        const map = new this(schema);
        const add = (key, value) => {
          if (typeof replacer === "function")
            value = replacer.call(obj, key, value);
          else if (Array.isArray(replacer) && !replacer.includes(key))
            return;
          if (value !== void 0 || keepUndefined)
            map.items.push(Pair.createPair(key, value, ctx));
        };
        if (obj instanceof Map) {
          for (const [key, value] of obj)
            add(key, value);
        } else if (obj && typeof obj === "object") {
          for (const key of Object.keys(obj))
            add(key, obj[key]);
        }
        if (typeof schema.sortMapEntries === "function") {
          map.items.sort(schema.sortMapEntries);
        }
        return map;
      }
      /**
       * Adds a value to the collection.
       *
       * @param overwrite - If not set `true`, using a key that is already in the
       *   collection will throw. Otherwise, overwrites the previous value.
       */
      add(pair, overwrite) {
        let _pair;
        if (identity.isPair(pair))
          _pair = pair;
        else if (!pair || typeof pair !== "object" || !("key" in pair)) {
          _pair = new Pair.Pair(pair, pair?.value);
        } else
          _pair = new Pair.Pair(pair.key, pair.value);
        const prev = findPair(this.items, _pair.key);
        const sortEntries = this.schema?.sortMapEntries;
        if (prev) {
          if (!overwrite)
            throw new Error(`Key ${_pair.key} already set`);
          if (identity.isScalar(prev.value) && Scalar.isScalarValue(_pair.value))
            prev.value.value = _pair.value;
          else
            prev.value = _pair.value;
        } else if (sortEntries) {
          const i = this.items.findIndex((item) => sortEntries(_pair, item) < 0);
          if (i === -1)
            this.items.push(_pair);
          else
            this.items.splice(i, 0, _pair);
        } else {
          this.items.push(_pair);
        }
      }
      delete(key) {
        const it = findPair(this.items, key);
        if (!it)
          return false;
        const del = this.items.splice(this.items.indexOf(it), 1);
        return del.length > 0;
      }
      get(key, keepScalar) {
        const it = findPair(this.items, key);
        const node = it?.value;
        return (!keepScalar && identity.isScalar(node) ? node.value : node) ?? void 0;
      }
      has(key) {
        return !!findPair(this.items, key);
      }
      set(key, value) {
        this.add(new Pair.Pair(key, value), true);
      }
      /**
       * @param ctx - Conversion context, originally set in Document#toJS()
       * @param {Class} Type - If set, forces the returned collection type
       * @returns Instance of Type, Map, or Object
       */
      toJSON(_, ctx, Type) {
        const map = Type ? new Type() : ctx?.mapAsMap ? /* @__PURE__ */ new Map() : {};
        if (ctx?.onCreate)
          ctx.onCreate(map);
        for (const item of this.items)
          addPairToJSMap.addPairToJSMap(ctx, map, item);
        return map;
      }
      toString(ctx, onComment, onChompKeep) {
        if (!ctx)
          return JSON.stringify(this);
        for (const item of this.items) {
          if (!identity.isPair(item))
            throw new Error(`Map items must all be pairs; found ${JSON.stringify(item)} instead`);
        }
        if (!ctx.allNullValues && this.hasAllNullValues(false))
          ctx = Object.assign({}, ctx, { allNullValues: true });
        return stringifyCollection.stringifyCollection(this, ctx, {
          blockItemPrefix: "",
          flowChars: { start: "{", end: "}" },
          itemIndent: ctx.indent || "",
          onChompKeep,
          onComment
        });
      }
    };
    exports.YAMLMap = YAMLMap;
    exports.findPair = findPair;
  }
});

// node_modules/yaml/dist/schema/common/map.js
var require_map = __commonJS({
  "node_modules/yaml/dist/schema/common/map.js"(exports) {
    "use strict";
    var identity = require_identity();
    var YAMLMap = require_YAMLMap();
    var map = {
      collection: "map",
      default: true,
      nodeClass: YAMLMap.YAMLMap,
      tag: "tag:yaml.org,2002:map",
      resolve(map2, onError) {
        if (!identity.isMap(map2))
          onError("Expected a mapping for this tag");
        return map2;
      },
      createNode: (schema, obj, ctx) => YAMLMap.YAMLMap.from(schema, obj, ctx)
    };
    exports.map = map;
  }
});

// node_modules/yaml/dist/nodes/YAMLSeq.js
var require_YAMLSeq = __commonJS({
  "node_modules/yaml/dist/nodes/YAMLSeq.js"(exports) {
    "use strict";
    var createNode = require_createNode();
    var stringifyCollection = require_stringifyCollection();
    var Collection = require_Collection();
    var identity = require_identity();
    var Scalar = require_Scalar();
    var toJS = require_toJS();
    var YAMLSeq = class extends Collection.Collection {
      static get tagName() {
        return "tag:yaml.org,2002:seq";
      }
      constructor(schema) {
        super(identity.SEQ, schema);
        this.items = [];
      }
      add(value) {
        this.items.push(value);
      }
      /**
       * Removes a value from the collection.
       *
       * `key` must contain a representation of an integer for this to succeed.
       * It may be wrapped in a `Scalar`.
       *
       * @returns `true` if the item was found and removed.
       */
      delete(key) {
        const idx = asItemIndex(key);
        if (typeof idx !== "number")
          return false;
        const del = this.items.splice(idx, 1);
        return del.length > 0;
      }
      get(key, keepScalar) {
        const idx = asItemIndex(key);
        if (typeof idx !== "number")
          return void 0;
        const it = this.items[idx];
        return !keepScalar && identity.isScalar(it) ? it.value : it;
      }
      /**
       * Checks if the collection includes a value with the key `key`.
       *
       * `key` must contain a representation of an integer for this to succeed.
       * It may be wrapped in a `Scalar`.
       */
      has(key) {
        const idx = asItemIndex(key);
        return typeof idx === "number" && idx < this.items.length;
      }
      /**
       * Sets a value in this collection. For `!!set`, `value` needs to be a
       * boolean to add/remove the item from the set.
       *
       * If `key` does not contain a representation of an integer, this will throw.
       * It may be wrapped in a `Scalar`.
       */
      set(key, value) {
        const idx = asItemIndex(key);
        if (typeof idx !== "number")
          throw new Error(`Expected a valid index, not ${key}.`);
        const prev = this.items[idx];
        if (identity.isScalar(prev) && Scalar.isScalarValue(value))
          prev.value = value;
        else
          this.items[idx] = value;
      }
      toJSON(_, ctx) {
        const seq = [];
        if (ctx?.onCreate)
          ctx.onCreate(seq);
        let i = 0;
        for (const item of this.items)
          seq.push(toJS.toJS(item, String(i++), ctx));
        return seq;
      }
      toString(ctx, onComment, onChompKeep) {
        if (!ctx)
          return JSON.stringify(this);
        return stringifyCollection.stringifyCollection(this, ctx, {
          blockItemPrefix: "- ",
          flowChars: { start: "[", end: "]" },
          itemIndent: (ctx.indent || "") + "  ",
          onChompKeep,
          onComment
        });
      }
      static from(schema, obj, ctx) {
        const { replacer } = ctx;
        const seq = new this(schema);
        if (obj && Symbol.iterator in Object(obj)) {
          let i = 0;
          for (let it of obj) {
            if (typeof replacer === "function") {
              const key = obj instanceof Set ? it : String(i++);
              it = replacer.call(obj, key, it);
            }
            seq.items.push(createNode.createNode(it, void 0, ctx));
          }
        }
        return seq;
      }
    };
    function asItemIndex(key) {
      let idx = identity.isScalar(key) ? key.value : key;
      if (idx && typeof idx === "string")
        idx = Number(idx);
      return typeof idx === "number" && Number.isInteger(idx) && idx >= 0 ? idx : null;
    }
    exports.YAMLSeq = YAMLSeq;
  }
});

// node_modules/yaml/dist/schema/common/seq.js
var require_seq = __commonJS({
  "node_modules/yaml/dist/schema/common/seq.js"(exports) {
    "use strict";
    var identity = require_identity();
    var YAMLSeq = require_YAMLSeq();
    var seq = {
      collection: "seq",
      default: true,
      nodeClass: YAMLSeq.YAMLSeq,
      tag: "tag:yaml.org,2002:seq",
      resolve(seq2, onError) {
        if (!identity.isSeq(seq2))
          onError("Expected a sequence for this tag");
        return seq2;
      },
      createNode: (schema, obj, ctx) => YAMLSeq.YAMLSeq.from(schema, obj, ctx)
    };
    exports.seq = seq;
  }
});

// node_modules/yaml/dist/schema/common/string.js
var require_string = __commonJS({
  "node_modules/yaml/dist/schema/common/string.js"(exports) {
    "use strict";
    var stringifyString = require_stringifyString();
    var string = {
      identify: (value) => typeof value === "string",
      default: true,
      tag: "tag:yaml.org,2002:str",
      resolve: (str) => str,
      stringify(item, ctx, onComment, onChompKeep) {
        ctx = Object.assign({ actualString: true }, ctx);
        return stringifyString.stringifyString(item, ctx, onComment, onChompKeep);
      }
    };
    exports.string = string;
  }
});

// node_modules/yaml/dist/schema/common/null.js
var require_null = __commonJS({
  "node_modules/yaml/dist/schema/common/null.js"(exports) {
    "use strict";
    var Scalar = require_Scalar();
    var nullTag = {
      identify: (value) => value == null,
      createNode: () => new Scalar.Scalar(null),
      default: true,
      tag: "tag:yaml.org,2002:null",
      test: /^(?:~|[Nn]ull|NULL)?$/,
      resolve: () => new Scalar.Scalar(null),
      stringify: ({ source }, ctx) => typeof source === "string" && nullTag.test.test(source) ? source : ctx.options.nullStr
    };
    exports.nullTag = nullTag;
  }
});

// node_modules/yaml/dist/schema/core/bool.js
var require_bool = __commonJS({
  "node_modules/yaml/dist/schema/core/bool.js"(exports) {
    "use strict";
    var Scalar = require_Scalar();
    var boolTag = {
      identify: (value) => typeof value === "boolean",
      default: true,
      tag: "tag:yaml.org,2002:bool",
      test: /^(?:[Tt]rue|TRUE|[Ff]alse|FALSE)$/,
      resolve: (str) => new Scalar.Scalar(str[0] === "t" || str[0] === "T"),
      stringify({ source, value }, ctx) {
        if (source && boolTag.test.test(source)) {
          const sv = source[0] === "t" || source[0] === "T";
          if (value === sv)
            return source;
        }
        return value ? ctx.options.trueStr : ctx.options.falseStr;
      }
    };
    exports.boolTag = boolTag;
  }
});

// node_modules/yaml/dist/stringify/stringifyNumber.js
var require_stringifyNumber = __commonJS({
  "node_modules/yaml/dist/stringify/stringifyNumber.js"(exports) {
    "use strict";
    function stringifyNumber({ format, minFractionDigits, tag, value }) {
      if (typeof value === "bigint")
        return String(value);
      const num = typeof value === "number" ? value : Number(value);
      if (!isFinite(num))
        return isNaN(num) ? ".nan" : num < 0 ? "-.inf" : ".inf";
      let n = Object.is(value, -0) ? "-0" : JSON.stringify(value);
      if (!format && minFractionDigits && (!tag || tag === "tag:yaml.org,2002:float") && /^-?\d/.test(n) && !n.includes("e")) {
        let i = n.indexOf(".");
        if (i < 0) {
          i = n.length;
          n += ".";
        }
        let d = minFractionDigits - (n.length - i - 1);
        while (d-- > 0)
          n += "0";
      }
      return n;
    }
    exports.stringifyNumber = stringifyNumber;
  }
});

// node_modules/yaml/dist/schema/core/float.js
var require_float = __commonJS({
  "node_modules/yaml/dist/schema/core/float.js"(exports) {
    "use strict";
    var Scalar = require_Scalar();
    var stringifyNumber = require_stringifyNumber();
    var floatNaN = {
      identify: (value) => typeof value === "number",
      default: true,
      tag: "tag:yaml.org,2002:float",
      test: /^(?:[-+]?\.(?:inf|Inf|INF)|\.nan|\.NaN|\.NAN)$/,
      resolve: (str) => str.slice(-3).toLowerCase() === "nan" ? NaN : str[0] === "-" ? Number.NEGATIVE_INFINITY : Number.POSITIVE_INFINITY,
      stringify: stringifyNumber.stringifyNumber
    };
    var floatExp = {
      identify: (value) => typeof value === "number",
      default: true,
      tag: "tag:yaml.org,2002:float",
      format: "EXP",
      test: /^[-+]?(?:\.[0-9]+|[0-9]+(?:\.[0-9]*)?)[eE][-+]?[0-9]+$/,
      resolve: (str) => parseFloat(str),
      stringify(node) {
        const num = Number(node.value);
        return isFinite(num) ? num.toExponential() : stringifyNumber.stringifyNumber(node);
      }
    };
    var float = {
      identify: (value) => typeof value === "number",
      default: true,
      tag: "tag:yaml.org,2002:float",
      test: /^[-+]?(?:\.[0-9]+|[0-9]+\.[0-9]*)$/,
      resolve(str) {
        const node = new Scalar.Scalar(parseFloat(str));
        const dot = str.indexOf(".");
        if (dot !== -1 && str[str.length - 1] === "0")
          node.minFractionDigits = str.length - dot - 1;
        return node;
      },
      stringify: stringifyNumber.stringifyNumber
    };
    exports.float = float;
    exports.floatExp = floatExp;
    exports.floatNaN = floatNaN;
  }
});

// node_modules/yaml/dist/schema/core/int.js
var require_int = __commonJS({
  "node_modules/yaml/dist/schema/core/int.js"(exports) {
    "use strict";
    var stringifyNumber = require_stringifyNumber();
    var intIdentify = (value) => typeof value === "bigint" || Number.isInteger(value);
    var intResolve = (str, offset, radix, { intAsBigInt }) => intAsBigInt ? BigInt(str) : parseInt(str.substring(offset), radix);
    function intStringify(node, radix, prefix) {
      const { value } = node;
      if (intIdentify(value) && value >= 0)
        return prefix + value.toString(radix);
      return stringifyNumber.stringifyNumber(node);
    }
    var intOct = {
      identify: (value) => intIdentify(value) && value >= 0,
      default: true,
      tag: "tag:yaml.org,2002:int",
      format: "OCT",
      test: /^0o[0-7]+$/,
      resolve: (str, _onError, opt) => intResolve(str, 2, 8, opt),
      stringify: (node) => intStringify(node, 8, "0o")
    };
    var int = {
      identify: intIdentify,
      default: true,
      tag: "tag:yaml.org,2002:int",
      test: /^[-+]?[0-9]+$/,
      resolve: (str, _onError, opt) => intResolve(str, 0, 10, opt),
      stringify: stringifyNumber.stringifyNumber
    };
    var intHex = {
      identify: (value) => intIdentify(value) && value >= 0,
      default: true,
      tag: "tag:yaml.org,2002:int",
      format: "HEX",
      test: /^0x[0-9a-fA-F]+$/,
      resolve: (str, _onError, opt) => intResolve(str, 2, 16, opt),
      stringify: (node) => intStringify(node, 16, "0x")
    };
    exports.int = int;
    exports.intHex = intHex;
    exports.intOct = intOct;
  }
});

// node_modules/yaml/dist/schema/core/schema.js
var require_schema = __commonJS({
  "node_modules/yaml/dist/schema/core/schema.js"(exports) {
    "use strict";
    var map = require_map();
    var _null = require_null();
    var seq = require_seq();
    var string = require_string();
    var bool = require_bool();
    var float = require_float();
    var int = require_int();
    var schema = [
      map.map,
      seq.seq,
      string.string,
      _null.nullTag,
      bool.boolTag,
      int.intOct,
      int.int,
      int.intHex,
      float.floatNaN,
      float.floatExp,
      float.float
    ];
    exports.schema = schema;
  }
});

// node_modules/yaml/dist/schema/json/schema.js
var require_schema2 = __commonJS({
  "node_modules/yaml/dist/schema/json/schema.js"(exports) {
    "use strict";
    var Scalar = require_Scalar();
    var map = require_map();
    var seq = require_seq();
    function intIdentify(value) {
      return typeof value === "bigint" || Number.isInteger(value);
    }
    var stringifyJSON = ({ value }) => JSON.stringify(value);
    var jsonScalars = [
      {
        identify: (value) => typeof value === "string",
        default: true,
        tag: "tag:yaml.org,2002:str",
        resolve: (str) => str,
        stringify: stringifyJSON
      },
      {
        identify: (value) => value == null,
        createNode: () => new Scalar.Scalar(null),
        default: true,
        tag: "tag:yaml.org,2002:null",
        test: /^null$/,
        resolve: () => null,
        stringify: stringifyJSON
      },
      {
        identify: (value) => typeof value === "boolean",
        default: true,
        tag: "tag:yaml.org,2002:bool",
        test: /^true$|^false$/,
        resolve: (str) => str === "true",
        stringify: stringifyJSON
      },
      {
        identify: intIdentify,
        default: true,
        tag: "tag:yaml.org,2002:int",
        test: /^-?(?:0|[1-9][0-9]*)$/,
        resolve: (str, _onError, { intAsBigInt }) => intAsBigInt ? BigInt(str) : parseInt(str, 10),
        stringify: ({ value }) => intIdentify(value) ? value.toString() : JSON.stringify(value)
      },
      {
        identify: (value) => typeof value === "number",
        default: true,
        tag: "tag:yaml.org,2002:float",
        test: /^-?(?:0|[1-9][0-9]*)(?:\.[0-9]*)?(?:[eE][-+]?[0-9]+)?$/,
        resolve: (str) => parseFloat(str),
        stringify: stringifyJSON
      }
    ];
    var jsonError = {
      default: true,
      tag: "",
      test: /^/,
      resolve(str, onError) {
        onError(`Unresolved plain scalar ${JSON.stringify(str)}`);
        return str;
      }
    };
    var schema = [map.map, seq.seq].concat(jsonScalars, jsonError);
    exports.schema = schema;
  }
});

// node_modules/yaml/dist/schema/yaml-1.1/binary.js
var require_binary = __commonJS({
  "node_modules/yaml/dist/schema/yaml-1.1/binary.js"(exports) {
    "use strict";
    var node_buffer = __require("buffer");
    var Scalar = require_Scalar();
    var stringifyString = require_stringifyString();
    var binary = {
      identify: (value) => value instanceof Uint8Array,
      // Buffer inherits from Uint8Array
      default: false,
      tag: "tag:yaml.org,2002:binary",
      /**
       * Returns a Buffer in node and an Uint8Array in browsers
       *
       * To use the resulting buffer as an image, you'll want to do something like:
       *
       *   const blob = new Blob([buffer], { type: 'image/jpeg' })
       *   document.querySelector('#photo').src = URL.createObjectURL(blob)
       */
      resolve(src, onError) {
        if (typeof node_buffer.Buffer === "function") {
          return node_buffer.Buffer.from(src, "base64");
        } else if (typeof atob === "function") {
          const str = atob(src.replace(/[\n\r]/g, ""));
          const buffer = new Uint8Array(str.length);
          for (let i = 0; i < str.length; ++i)
            buffer[i] = str.charCodeAt(i);
          return buffer;
        } else {
          onError("This environment does not support reading binary tags; either Buffer or atob is required");
          return src;
        }
      },
      stringify({ comment, type, value }, ctx, onComment, onChompKeep) {
        if (!value)
          return "";
        const buf = value;
        let str;
        if (typeof node_buffer.Buffer === "function") {
          str = buf instanceof node_buffer.Buffer ? buf.toString("base64") : node_buffer.Buffer.from(buf.buffer).toString("base64");
        } else if (typeof btoa === "function") {
          let s = "";
          for (let i = 0; i < buf.length; ++i)
            s += String.fromCharCode(buf[i]);
          str = btoa(s);
        } else {
          throw new Error("This environment does not support writing binary tags; either Buffer or btoa is required");
        }
        type ?? (type = Scalar.Scalar.BLOCK_LITERAL);
        if (type !== Scalar.Scalar.QUOTE_DOUBLE) {
          const lineWidth = Math.max(ctx.options.lineWidth - ctx.indent.length, ctx.options.minContentWidth);
          const n = Math.ceil(str.length / lineWidth);
          const lines = new Array(n);
          for (let i = 0, o = 0; i < n; ++i, o += lineWidth) {
            lines[i] = str.substr(o, lineWidth);
          }
          str = lines.join(type === Scalar.Scalar.BLOCK_LITERAL ? "\n" : " ");
        }
        return stringifyString.stringifyString({ comment, type, value: str }, ctx, onComment, onChompKeep);
      }
    };
    exports.binary = binary;
  }
});

// node_modules/yaml/dist/schema/yaml-1.1/pairs.js
var require_pairs = __commonJS({
  "node_modules/yaml/dist/schema/yaml-1.1/pairs.js"(exports) {
    "use strict";
    var identity = require_identity();
    var Pair = require_Pair();
    var Scalar = require_Scalar();
    var YAMLSeq = require_YAMLSeq();
    function resolvePairs(seq, onError) {
      if (identity.isSeq(seq)) {
        for (let i = 0; i < seq.items.length; ++i) {
          let item = seq.items[i];
          if (identity.isPair(item))
            continue;
          else if (identity.isMap(item)) {
            if (item.items.length > 1)
              onError("Each pair must have its own sequence indicator");
            const pair = item.items[0] || new Pair.Pair(new Scalar.Scalar(null));
            if (item.commentBefore)
              pair.key.commentBefore = pair.key.commentBefore ? `${item.commentBefore}
${pair.key.commentBefore}` : item.commentBefore;
            if (item.comment) {
              const cn = pair.value ?? pair.key;
              cn.comment = cn.comment ? `${item.comment}
${cn.comment}` : item.comment;
            }
            item = pair;
          }
          seq.items[i] = identity.isPair(item) ? item : new Pair.Pair(item);
        }
      } else
        onError("Expected a sequence for this tag");
      return seq;
    }
    function createPairs(schema, iterable, ctx) {
      const { replacer } = ctx;
      const pairs2 = new YAMLSeq.YAMLSeq(schema);
      pairs2.tag = "tag:yaml.org,2002:pairs";
      let i = 0;
      if (iterable && Symbol.iterator in Object(iterable))
        for (let it of iterable) {
          if (typeof replacer === "function")
            it = replacer.call(iterable, String(i++), it);
          let key, value;
          if (Array.isArray(it)) {
            if (it.length === 2) {
              key = it[0];
              value = it[1];
            } else
              throw new TypeError(`Expected [key, value] tuple: ${it}`);
          } else if (it && it instanceof Object) {
            const keys = Object.keys(it);
            if (keys.length === 1) {
              key = keys[0];
              value = it[key];
            } else {
              throw new TypeError(`Expected tuple with one key, not ${keys.length} keys`);
            }
          } else {
            key = it;
          }
          pairs2.items.push(Pair.createPair(key, value, ctx));
        }
      return pairs2;
    }
    var pairs = {
      collection: "seq",
      default: false,
      tag: "tag:yaml.org,2002:pairs",
      resolve: resolvePairs,
      createNode: createPairs
    };
    exports.createPairs = createPairs;
    exports.pairs = pairs;
    exports.resolvePairs = resolvePairs;
  }
});

// node_modules/yaml/dist/schema/yaml-1.1/omap.js
var require_omap = __commonJS({
  "node_modules/yaml/dist/schema/yaml-1.1/omap.js"(exports) {
    "use strict";
    var identity = require_identity();
    var toJS = require_toJS();
    var YAMLMap = require_YAMLMap();
    var YAMLSeq = require_YAMLSeq();
    var pairs = require_pairs();
    var YAMLOMap = class _YAMLOMap extends YAMLSeq.YAMLSeq {
      constructor() {
        super();
        this.add = YAMLMap.YAMLMap.prototype.add.bind(this);
        this.delete = YAMLMap.YAMLMap.prototype.delete.bind(this);
        this.get = YAMLMap.YAMLMap.prototype.get.bind(this);
        this.has = YAMLMap.YAMLMap.prototype.has.bind(this);
        this.set = YAMLMap.YAMLMap.prototype.set.bind(this);
        this.tag = _YAMLOMap.tag;
      }
      /**
       * If `ctx` is given, the return type is actually `Map<unknown, unknown>`,
       * but TypeScript won't allow widening the signature of a child method.
       */
      toJSON(_, ctx) {
        if (!ctx)
          return super.toJSON(_);
        const map = /* @__PURE__ */ new Map();
        if (ctx?.onCreate)
          ctx.onCreate(map);
        for (const pair of this.items) {
          let key, value;
          if (identity.isPair(pair)) {
            key = toJS.toJS(pair.key, "", ctx);
            value = toJS.toJS(pair.value, key, ctx);
          } else {
            key = toJS.toJS(pair, "", ctx);
          }
          if (map.has(key))
            throw new Error("Ordered maps must not include duplicate keys");
          map.set(key, value);
        }
        return map;
      }
      static from(schema, iterable, ctx) {
        const pairs$1 = pairs.createPairs(schema, iterable, ctx);
        const omap2 = new this();
        omap2.items = pairs$1.items;
        return omap2;
      }
    };
    YAMLOMap.tag = "tag:yaml.org,2002:omap";
    var omap = {
      collection: "seq",
      identify: (value) => value instanceof Map,
      nodeClass: YAMLOMap,
      default: false,
      tag: "tag:yaml.org,2002:omap",
      resolve(seq, onError) {
        const pairs$1 = pairs.resolvePairs(seq, onError);
        const seenKeys = [];
        for (const { key } of pairs$1.items) {
          if (identity.isScalar(key)) {
            if (seenKeys.includes(key.value)) {
              onError(`Ordered maps must not include duplicate keys: ${key.value}`);
            } else {
              seenKeys.push(key.value);
            }
          }
        }
        return Object.assign(new YAMLOMap(), pairs$1);
      },
      createNode: (schema, iterable, ctx) => YAMLOMap.from(schema, iterable, ctx)
    };
    exports.YAMLOMap = YAMLOMap;
    exports.omap = omap;
  }
});

// node_modules/yaml/dist/schema/yaml-1.1/bool.js
var require_bool2 = __commonJS({
  "node_modules/yaml/dist/schema/yaml-1.1/bool.js"(exports) {
    "use strict";
    var Scalar = require_Scalar();
    function boolStringify({ value, source }, ctx) {
      const boolObj = value ? trueTag : falseTag;
      if (source && boolObj.test.test(source))
        return source;
      return value ? ctx.options.trueStr : ctx.options.falseStr;
    }
    var trueTag = {
      identify: (value) => value === true,
      default: true,
      tag: "tag:yaml.org,2002:bool",
      test: /^(?:Y|y|[Yy]es|YES|[Tt]rue|TRUE|[Oo]n|ON)$/,
      resolve: () => new Scalar.Scalar(true),
      stringify: boolStringify
    };
    var falseTag = {
      identify: (value) => value === false,
      default: true,
      tag: "tag:yaml.org,2002:bool",
      test: /^(?:N|n|[Nn]o|NO|[Ff]alse|FALSE|[Oo]ff|OFF)$/,
      resolve: () => new Scalar.Scalar(false),
      stringify: boolStringify
    };
    exports.falseTag = falseTag;
    exports.trueTag = trueTag;
  }
});

// node_modules/yaml/dist/schema/yaml-1.1/float.js
var require_float2 = __commonJS({
  "node_modules/yaml/dist/schema/yaml-1.1/float.js"(exports) {
    "use strict";
    var Scalar = require_Scalar();
    var stringifyNumber = require_stringifyNumber();
    var floatNaN = {
      identify: (value) => typeof value === "number",
      default: true,
      tag: "tag:yaml.org,2002:float",
      test: /^(?:[-+]?\.(?:inf|Inf|INF)|\.nan|\.NaN|\.NAN)$/,
      resolve: (str) => str.slice(-3).toLowerCase() === "nan" ? NaN : str[0] === "-" ? Number.NEGATIVE_INFINITY : Number.POSITIVE_INFINITY,
      stringify: stringifyNumber.stringifyNumber
    };
    var floatExp = {
      identify: (value) => typeof value === "number",
      default: true,
      tag: "tag:yaml.org,2002:float",
      format: "EXP",
      test: /^[-+]?(?:[0-9][0-9_]*)?(?:\.[0-9_]*)?[eE][-+]?[0-9]+$/,
      resolve: (str) => parseFloat(str.replace(/_/g, "")),
      stringify(node) {
        const num = Number(node.value);
        return isFinite(num) ? num.toExponential() : stringifyNumber.stringifyNumber(node);
      }
    };
    var float = {
      identify: (value) => typeof value === "number",
      default: true,
      tag: "tag:yaml.org,2002:float",
      test: /^[-+]?(?:[0-9][0-9_]*)?\.[0-9_]*$/,
      resolve(str) {
        const node = new Scalar.Scalar(parseFloat(str.replace(/_/g, "")));
        const dot = str.indexOf(".");
        if (dot !== -1) {
          const f = str.substring(dot + 1).replace(/_/g, "");
          if (f[f.length - 1] === "0")
            node.minFractionDigits = f.length;
        }
        return node;
      },
      stringify: stringifyNumber.stringifyNumber
    };
    exports.float = float;
    exports.floatExp = floatExp;
    exports.floatNaN = floatNaN;
  }
});

// node_modules/yaml/dist/schema/yaml-1.1/int.js
var require_int2 = __commonJS({
  "node_modules/yaml/dist/schema/yaml-1.1/int.js"(exports) {
    "use strict";
    var stringifyNumber = require_stringifyNumber();
    var intIdentify = (value) => typeof value === "bigint" || Number.isInteger(value);
    function intResolve(str, offset, radix, { intAsBigInt }) {
      const sign = str[0];
      if (sign === "-" || sign === "+")
        offset += 1;
      str = str.substring(offset).replace(/_/g, "");
      if (intAsBigInt) {
        switch (radix) {
          case 2:
            str = `0b${str}`;
            break;
          case 8:
            str = `0o${str}`;
            break;
          case 16:
            str = `0x${str}`;
            break;
        }
        const n2 = BigInt(str);
        return sign === "-" ? BigInt(-1) * n2 : n2;
      }
      const n = parseInt(str, radix);
      return sign === "-" ? -1 * n : n;
    }
    function intStringify(node, radix, prefix) {
      const { value } = node;
      if (intIdentify(value)) {
        const str = value.toString(radix);
        return value < 0 ? "-" + prefix + str.substr(1) : prefix + str;
      }
      return stringifyNumber.stringifyNumber(node);
    }
    var intBin = {
      identify: intIdentify,
      default: true,
      tag: "tag:yaml.org,2002:int",
      format: "BIN",
      test: /^[-+]?0b[0-1_]+$/,
      resolve: (str, _onError, opt) => intResolve(str, 2, 2, opt),
      stringify: (node) => intStringify(node, 2, "0b")
    };
    var intOct = {
      identify: intIdentify,
      default: true,
      tag: "tag:yaml.org,2002:int",
      format: "OCT",
      test: /^[-+]?0[0-7_]+$/,
      resolve: (str, _onError, opt) => intResolve(str, 1, 8, opt),
      stringify: (node) => intStringify(node, 8, "0")
    };
    var int = {
      identify: intIdentify,
      default: true,
      tag: "tag:yaml.org,2002:int",
      test: /^[-+]?[0-9][0-9_]*$/,
      resolve: (str, _onError, opt) => intResolve(str, 0, 10, opt),
      stringify: stringifyNumber.stringifyNumber
    };
    var intHex = {
      identify: intIdentify,
      default: true,
      tag: "tag:yaml.org,2002:int",
      format: "HEX",
      test: /^[-+]?0x[0-9a-fA-F_]+$/,
      resolve: (str, _onError, opt) => intResolve(str, 2, 16, opt),
      stringify: (node) => intStringify(node, 16, "0x")
    };
    exports.int = int;
    exports.intBin = intBin;
    exports.intHex = intHex;
    exports.intOct = intOct;
  }
});

// node_modules/yaml/dist/schema/yaml-1.1/set.js
var require_set = __commonJS({
  "node_modules/yaml/dist/schema/yaml-1.1/set.js"(exports) {
    "use strict";
    var identity = require_identity();
    var Pair = require_Pair();
    var YAMLMap = require_YAMLMap();
    var YAMLSet = class _YAMLSet extends YAMLMap.YAMLMap {
      constructor(schema) {
        super(schema);
        this.tag = _YAMLSet.tag;
      }
      add(key) {
        let pair;
        if (identity.isPair(key))
          pair = key;
        else if (key && typeof key === "object" && "key" in key && "value" in key && key.value === null)
          pair = new Pair.Pair(key.key, null);
        else
          pair = new Pair.Pair(key, null);
        const prev = YAMLMap.findPair(this.items, pair.key);
        if (!prev)
          this.items.push(pair);
      }
      /**
       * If `keepPair` is `true`, returns the Pair matching `key`.
       * Otherwise, returns the value of that Pair's key.
       */
      get(key, keepPair) {
        const pair = YAMLMap.findPair(this.items, key);
        return !keepPair && identity.isPair(pair) ? identity.isScalar(pair.key) ? pair.key.value : pair.key : pair;
      }
      set(key, value) {
        if (typeof value !== "boolean")
          throw new Error(`Expected boolean value for set(key, value) in a YAML set, not ${typeof value}`);
        const prev = YAMLMap.findPair(this.items, key);
        if (prev && !value) {
          this.items.splice(this.items.indexOf(prev), 1);
        } else if (!prev && value) {
          this.items.push(new Pair.Pair(key));
        }
      }
      toJSON(_, ctx) {
        return super.toJSON(_, ctx, Set);
      }
      toString(ctx, onComment, onChompKeep) {
        if (!ctx)
          return JSON.stringify(this);
        if (this.hasAllNullValues(true))
          return super.toString(Object.assign({}, ctx, { allNullValues: true }), onComment, onChompKeep);
        else
          throw new Error("Set items must all have null values");
      }
      static from(schema, iterable, ctx) {
        const { replacer } = ctx;
        const set2 = new this(schema);
        if (iterable && Symbol.iterator in Object(iterable))
          for (let value of iterable) {
            if (typeof replacer === "function")
              value = replacer.call(iterable, value, value);
            set2.items.push(Pair.createPair(value, null, ctx));
          }
        return set2;
      }
    };
    YAMLSet.tag = "tag:yaml.org,2002:set";
    var set = {
      collection: "map",
      identify: (value) => value instanceof Set,
      nodeClass: YAMLSet,
      default: false,
      tag: "tag:yaml.org,2002:set",
      createNode: (schema, iterable, ctx) => YAMLSet.from(schema, iterable, ctx),
      resolve(map, onError) {
        if (identity.isMap(map)) {
          if (map.hasAllNullValues(true))
            return Object.assign(new YAMLSet(), map);
          else
            onError("Set items must all have null values");
        } else
          onError("Expected a mapping for this tag");
        return map;
      }
    };
    exports.YAMLSet = YAMLSet;
    exports.set = set;
  }
});

// node_modules/yaml/dist/schema/yaml-1.1/timestamp.js
var require_timestamp = __commonJS({
  "node_modules/yaml/dist/schema/yaml-1.1/timestamp.js"(exports) {
    "use strict";
    var stringifyNumber = require_stringifyNumber();
    function parseSexagesimal(str, asBigInt) {
      const sign = str[0];
      const parts = sign === "-" || sign === "+" ? str.substring(1) : str;
      const num = (n) => asBigInt ? BigInt(n) : Number(n);
      const res = parts.replace(/_/g, "").split(":").reduce((res2, p) => res2 * num(60) + num(p), num(0));
      return sign === "-" ? num(-1) * res : res;
    }
    function stringifySexagesimal(node) {
      let { value } = node;
      let num = (n) => n;
      if (typeof value === "bigint")
        num = (n) => BigInt(n);
      else if (isNaN(value) || !isFinite(value))
        return stringifyNumber.stringifyNumber(node);
      let sign = "";
      if (value < 0) {
        sign = "-";
        value *= num(-1);
      }
      const _60 = num(60);
      const parts = [value % _60];
      if (value < 60) {
        parts.unshift(0);
      } else {
        value = (value - parts[0]) / _60;
        parts.unshift(value % _60);
        if (value >= 60) {
          value = (value - parts[0]) / _60;
          parts.unshift(value);
        }
      }
      return sign + parts.map((n) => String(n).padStart(2, "0")).join(":").replace(/000000\d*$/, "");
    }
    var intTime = {
      identify: (value) => typeof value === "bigint" || Number.isInteger(value),
      default: true,
      tag: "tag:yaml.org,2002:int",
      format: "TIME",
      test: /^[-+]?[0-9][0-9_]*(?::[0-5]?[0-9])+$/,
      resolve: (str, _onError, { intAsBigInt }) => parseSexagesimal(str, intAsBigInt),
      stringify: stringifySexagesimal
    };
    var floatTime = {
      identify: (value) => typeof value === "number",
      default: true,
      tag: "tag:yaml.org,2002:float",
      format: "TIME",
      test: /^[-+]?[0-9][0-9_]*(?::[0-5]?[0-9])+\.[0-9_]*$/,
      resolve: (str) => parseSexagesimal(str, false),
      stringify: stringifySexagesimal
    };
    var timestamp = {
      identify: (value) => value instanceof Date,
      default: true,
      tag: "tag:yaml.org,2002:timestamp",
      // If the time zone is omitted, the timestamp is assumed to be specified in UTC. The time part
      // may be omitted altogether, resulting in a date format. In such a case, the time part is
      // assumed to be 00:00:00Z (start of day, UTC).
      test: RegExp("^([0-9]{4})-([0-9]{1,2})-([0-9]{1,2})(?:(?:t|T|[ \\t]+)([0-9]{1,2}):([0-9]{1,2}):([0-9]{1,2}(\\.[0-9]+)?)(?:[ \\t]*(Z|[-+][012]?[0-9](?::[0-9]{2})?))?)?$"),
      resolve(str) {
        const match = str.match(timestamp.test);
        if (!match)
          throw new Error("!!timestamp expects a date, starting with yyyy-mm-dd");
        const [, year, month, day, hour, minute, second] = match.map(Number);
        const millisec = match[7] ? Number((match[7] + "00").substr(1, 3)) : 0;
        let date = Date.UTC(year, month - 1, day, hour || 0, minute || 0, second || 0, millisec);
        const tz = match[8];
        if (tz && tz !== "Z") {
          let d = parseSexagesimal(tz, false);
          if (Math.abs(d) < 30)
            d *= 60;
          date -= 6e4 * d;
        }
        return new Date(date);
      },
      stringify: ({ value }) => value?.toISOString().replace(/(T00:00:00)?\.000Z$/, "") ?? ""
    };
    exports.floatTime = floatTime;
    exports.intTime = intTime;
    exports.timestamp = timestamp;
  }
});

// node_modules/yaml/dist/schema/yaml-1.1/schema.js
var require_schema3 = __commonJS({
  "node_modules/yaml/dist/schema/yaml-1.1/schema.js"(exports) {
    "use strict";
    var map = require_map();
    var _null = require_null();
    var seq = require_seq();
    var string = require_string();
    var binary = require_binary();
    var bool = require_bool2();
    var float = require_float2();
    var int = require_int2();
    var merge = require_merge();
    var omap = require_omap();
    var pairs = require_pairs();
    var set = require_set();
    var timestamp = require_timestamp();
    var schema = [
      map.map,
      seq.seq,
      string.string,
      _null.nullTag,
      bool.trueTag,
      bool.falseTag,
      int.intBin,
      int.intOct,
      int.int,
      int.intHex,
      float.floatNaN,
      float.floatExp,
      float.float,
      binary.binary,
      merge.merge,
      omap.omap,
      pairs.pairs,
      set.set,
      timestamp.intTime,
      timestamp.floatTime,
      timestamp.timestamp
    ];
    exports.schema = schema;
  }
});

// node_modules/yaml/dist/schema/tags.js
var require_tags = __commonJS({
  "node_modules/yaml/dist/schema/tags.js"(exports) {
    "use strict";
    var map = require_map();
    var _null = require_null();
    var seq = require_seq();
    var string = require_string();
    var bool = require_bool();
    var float = require_float();
    var int = require_int();
    var schema = require_schema();
    var schema$1 = require_schema2();
    var binary = require_binary();
    var merge = require_merge();
    var omap = require_omap();
    var pairs = require_pairs();
    var schema$2 = require_schema3();
    var set = require_set();
    var timestamp = require_timestamp();
    var schemas = /* @__PURE__ */ new Map([
      ["core", schema.schema],
      ["failsafe", [map.map, seq.seq, string.string]],
      ["json", schema$1.schema],
      ["yaml11", schema$2.schema],
      ["yaml-1.1", schema$2.schema]
    ]);
    var tagsByName = {
      binary: binary.binary,
      bool: bool.boolTag,
      float: float.float,
      floatExp: float.floatExp,
      floatNaN: float.floatNaN,
      floatTime: timestamp.floatTime,
      int: int.int,
      intHex: int.intHex,
      intOct: int.intOct,
      intTime: timestamp.intTime,
      map: map.map,
      merge: merge.merge,
      null: _null.nullTag,
      omap: omap.omap,
      pairs: pairs.pairs,
      seq: seq.seq,
      set: set.set,
      timestamp: timestamp.timestamp
    };
    var coreKnownTags = {
      "tag:yaml.org,2002:binary": binary.binary,
      "tag:yaml.org,2002:merge": merge.merge,
      "tag:yaml.org,2002:omap": omap.omap,
      "tag:yaml.org,2002:pairs": pairs.pairs,
      "tag:yaml.org,2002:set": set.set,
      "tag:yaml.org,2002:timestamp": timestamp.timestamp
    };
    function getTags(customTags, schemaName, addMergeTag) {
      const schemaTags = schemas.get(schemaName);
      if (schemaTags && !customTags) {
        return addMergeTag && !schemaTags.includes(merge.merge) ? schemaTags.concat(merge.merge) : schemaTags.slice();
      }
      let tags = schemaTags;
      if (!tags) {
        if (Array.isArray(customTags))
          tags = [];
        else {
          const keys = Array.from(schemas.keys()).filter((key) => key !== "yaml11").map((key) => JSON.stringify(key)).join(", ");
          throw new Error(`Unknown schema "${schemaName}"; use one of ${keys} or define customTags array`);
        }
      }
      if (Array.isArray(customTags)) {
        for (const tag of customTags)
          tags = tags.concat(tag);
      } else if (typeof customTags === "function") {
        tags = customTags(tags.slice());
      }
      if (addMergeTag)
        tags = tags.concat(merge.merge);
      return tags.reduce((tags2, tag) => {
        const tagObj = typeof tag === "string" ? tagsByName[tag] : tag;
        if (!tagObj) {
          const tagName = JSON.stringify(tag);
          const keys = Object.keys(tagsByName).map((key) => JSON.stringify(key)).join(", ");
          throw new Error(`Unknown custom tag ${tagName}; use one of ${keys}`);
        }
        if (!tags2.includes(tagObj))
          tags2.push(tagObj);
        return tags2;
      }, []);
    }
    exports.coreKnownTags = coreKnownTags;
    exports.getTags = getTags;
  }
});

// node_modules/yaml/dist/schema/Schema.js
var require_Schema = __commonJS({
  "node_modules/yaml/dist/schema/Schema.js"(exports) {
    "use strict";
    var identity = require_identity();
    var map = require_map();
    var seq = require_seq();
    var string = require_string();
    var tags = require_tags();
    var sortMapEntriesByKey = (a, b) => a.key < b.key ? -1 : a.key > b.key ? 1 : 0;
    var Schema = class _Schema {
      constructor({ compat, customTags, merge, resolveKnownTags, schema, sortMapEntries, toStringDefaults }) {
        this.compat = Array.isArray(compat) ? tags.getTags(compat, "compat") : compat ? tags.getTags(null, compat) : null;
        this.name = typeof schema === "string" && schema || "core";
        this.knownTags = resolveKnownTags ? tags.coreKnownTags : {};
        this.tags = tags.getTags(customTags, this.name, merge);
        this.toStringOptions = toStringDefaults ?? null;
        Object.defineProperty(this, identity.MAP, { value: map.map });
        Object.defineProperty(this, identity.SCALAR, { value: string.string });
        Object.defineProperty(this, identity.SEQ, { value: seq.seq });
        this.sortMapEntries = typeof sortMapEntries === "function" ? sortMapEntries : sortMapEntries === true ? sortMapEntriesByKey : null;
      }
      clone() {
        const copy = Object.create(_Schema.prototype, Object.getOwnPropertyDescriptors(this));
        copy.tags = this.tags.slice();
        return copy;
      }
    };
    exports.Schema = Schema;
  }
});

// node_modules/yaml/dist/stringify/stringifyDocument.js
var require_stringifyDocument = __commonJS({
  "node_modules/yaml/dist/stringify/stringifyDocument.js"(exports) {
    "use strict";
    var identity = require_identity();
    var stringify2 = require_stringify();
    var stringifyComment = require_stringifyComment();
    function stringifyDocument(doc, options) {
      const lines = [];
      let hasDirectives = options.directives === true;
      if (options.directives !== false && doc.directives) {
        const dir = doc.directives.toString(doc);
        if (dir) {
          lines.push(dir);
          hasDirectives = true;
        } else if (doc.directives.docStart)
          hasDirectives = true;
      }
      if (hasDirectives)
        lines.push("---");
      const ctx = stringify2.createStringifyContext(doc, options);
      const { commentString } = ctx.options;
      if (doc.commentBefore) {
        if (lines.length !== 1)
          lines.unshift("");
        const cs = commentString(doc.commentBefore);
        lines.unshift(stringifyComment.indentComment(cs, ""));
      }
      let chompKeep = false;
      let contentComment = null;
      if (doc.contents) {
        if (identity.isNode(doc.contents)) {
          if (doc.contents.spaceBefore && hasDirectives)
            lines.push("");
          if (doc.contents.commentBefore) {
            const cs = commentString(doc.contents.commentBefore);
            lines.push(stringifyComment.indentComment(cs, ""));
          }
          ctx.forceBlockIndent = !!doc.comment;
          contentComment = doc.contents.comment;
        }
        const onChompKeep = contentComment ? void 0 : () => chompKeep = true;
        let body = stringify2.stringify(doc.contents, ctx, () => contentComment = null, onChompKeep);
        if (contentComment)
          body += stringifyComment.lineComment(body, "", commentString(contentComment));
        if ((body[0] === "|" || body[0] === ">") && lines[lines.length - 1] === "---") {
          lines[lines.length - 1] = `--- ${body}`;
        } else
          lines.push(body);
      } else {
        lines.push(stringify2.stringify(doc.contents, ctx));
      }
      if (doc.directives?.docEnd) {
        if (doc.comment) {
          const cs = commentString(doc.comment);
          if (cs.includes("\n")) {
            lines.push("...");
            lines.push(stringifyComment.indentComment(cs, ""));
          } else {
            lines.push(`... ${cs}`);
          }
        } else {
          lines.push("...");
        }
      } else {
        let dc = doc.comment;
        if (dc && chompKeep)
          dc = dc.replace(/^\n+/, "");
        if (dc) {
          if ((!chompKeep || contentComment) && lines[lines.length - 1] !== "")
            lines.push("");
          lines.push(stringifyComment.indentComment(commentString(dc), ""));
        }
      }
      return lines.join("\n") + "\n";
    }
    exports.stringifyDocument = stringifyDocument;
  }
});

// node_modules/yaml/dist/doc/Document.js
var require_Document = __commonJS({
  "node_modules/yaml/dist/doc/Document.js"(exports) {
    "use strict";
    var Alias = require_Alias();
    var Collection = require_Collection();
    var identity = require_identity();
    var Pair = require_Pair();
    var toJS = require_toJS();
    var Schema = require_Schema();
    var stringifyDocument = require_stringifyDocument();
    var anchors = require_anchors();
    var applyReviver = require_applyReviver();
    var createNode = require_createNode();
    var directives = require_directives();
    var Document = class _Document {
      constructor(value, replacer, options) {
        this.commentBefore = null;
        this.comment = null;
        this.errors = [];
        this.warnings = [];
        Object.defineProperty(this, identity.NODE_TYPE, { value: identity.DOC });
        let _replacer = null;
        if (typeof replacer === "function" || Array.isArray(replacer)) {
          _replacer = replacer;
        } else if (options === void 0 && replacer) {
          options = replacer;
          replacer = void 0;
        }
        const opt = Object.assign({
          intAsBigInt: false,
          keepSourceTokens: false,
          logLevel: "warn",
          prettyErrors: true,
          strict: true,
          stringKeys: false,
          uniqueKeys: true,
          version: "1.2"
        }, options);
        this.options = opt;
        let { version } = opt;
        if (options?._directives) {
          this.directives = options._directives.atDocument();
          if (this.directives.yaml.explicit)
            version = this.directives.yaml.version;
        } else
          this.directives = new directives.Directives({ version });
        this.setSchema(version, options);
        this.contents = value === void 0 ? null : this.createNode(value, _replacer, options);
      }
      /**
       * Create a deep copy of this Document and its contents.
       *
       * Custom Node values that inherit from `Object` still refer to their original instances.
       */
      clone() {
        const copy = Object.create(_Document.prototype, {
          [identity.NODE_TYPE]: { value: identity.DOC }
        });
        copy.commentBefore = this.commentBefore;
        copy.comment = this.comment;
        copy.errors = this.errors.slice();
        copy.warnings = this.warnings.slice();
        copy.options = Object.assign({}, this.options);
        if (this.directives)
          copy.directives = this.directives.clone();
        copy.schema = this.schema.clone();
        copy.contents = identity.isNode(this.contents) ? this.contents.clone(copy.schema) : this.contents;
        if (this.range)
          copy.range = this.range.slice();
        return copy;
      }
      /** Adds a value to the document. */
      add(value) {
        if (assertCollection(this.contents))
          this.contents.add(value);
      }
      /** Adds a value to the document. */
      addIn(path, value) {
        if (assertCollection(this.contents))
          this.contents.addIn(path, value);
      }
      /**
       * Create a new `Alias` node, ensuring that the target `node` has the required anchor.
       *
       * If `node` already has an anchor, `name` is ignored.
       * Otherwise, the `node.anchor` value will be set to `name`,
       * or if an anchor with that name is already present in the document,
       * `name` will be used as a prefix for a new unique anchor.
       * If `name` is undefined, the generated anchor will use 'a' as a prefix.
       */
      createAlias(node, name) {
        if (!node.anchor) {
          const prev = anchors.anchorNames(this);
          node.anchor = // eslint-disable-next-line @typescript-eslint/prefer-nullish-coalescing
          !name || prev.has(name) ? anchors.findNewAnchor(name || "a", prev) : name;
        }
        return new Alias.Alias(node.anchor);
      }
      createNode(value, replacer, options) {
        let _replacer = void 0;
        if (typeof replacer === "function") {
          value = replacer.call({ "": value }, "", value);
          _replacer = replacer;
        } else if (Array.isArray(replacer)) {
          const keyToStr = (v) => typeof v === "number" || v instanceof String || v instanceof Number;
          const asStr = replacer.filter(keyToStr).map(String);
          if (asStr.length > 0)
            replacer = replacer.concat(asStr);
          _replacer = replacer;
        } else if (options === void 0 && replacer) {
          options = replacer;
          replacer = void 0;
        }
        const { aliasDuplicateObjects, anchorPrefix, flow, keepUndefined, onTagObj, tag } = options ?? {};
        const { onAnchor, setAnchors, sourceObjects } = anchors.createNodeAnchors(
          this,
          // eslint-disable-next-line @typescript-eslint/prefer-nullish-coalescing
          anchorPrefix || "a"
        );
        const ctx = {
          aliasDuplicateObjects: aliasDuplicateObjects ?? true,
          keepUndefined: keepUndefined ?? false,
          onAnchor,
          onTagObj,
          replacer: _replacer,
          schema: this.schema,
          sourceObjects
        };
        const node = createNode.createNode(value, tag, ctx);
        if (flow && identity.isCollection(node))
          node.flow = true;
        setAnchors();
        return node;
      }
      /**
       * Convert a key and a value into a `Pair` using the current schema,
       * recursively wrapping all values as `Scalar` or `Collection` nodes.
       */
      createPair(key, value, options = {}) {
        const k = this.createNode(key, null, options);
        const v = this.createNode(value, null, options);
        return new Pair.Pair(k, v);
      }
      /**
       * Removes a value from the document.
       * @returns `true` if the item was found and removed.
       */
      delete(key) {
        return assertCollection(this.contents) ? this.contents.delete(key) : false;
      }
      /**
       * Removes a value from the document.
       * @returns `true` if the item was found and removed.
       */
      deleteIn(path) {
        if (Collection.isEmptyPath(path)) {
          if (this.contents == null)
            return false;
          this.contents = null;
          return true;
        }
        return assertCollection(this.contents) ? this.contents.deleteIn(path) : false;
      }
      /**
       * Returns item at `key`, or `undefined` if not found. By default unwraps
       * scalar values from their surrounding node; to disable set `keepScalar` to
       * `true` (collections are always returned intact).
       */
      get(key, keepScalar) {
        return identity.isCollection(this.contents) ? this.contents.get(key, keepScalar) : void 0;
      }
      /**
       * Returns item at `path`, or `undefined` if not found. By default unwraps
       * scalar values from their surrounding node; to disable set `keepScalar` to
       * `true` (collections are always returned intact).
       */
      getIn(path, keepScalar) {
        if (Collection.isEmptyPath(path))
          return !keepScalar && identity.isScalar(this.contents) ? this.contents.value : this.contents;
        return identity.isCollection(this.contents) ? this.contents.getIn(path, keepScalar) : void 0;
      }
      /**
       * Checks if the document includes a value with the key `key`.
       */
      has(key) {
        return identity.isCollection(this.contents) ? this.contents.has(key) : false;
      }
      /**
       * Checks if the document includes a value at `path`.
       */
      hasIn(path) {
        if (Collection.isEmptyPath(path))
          return this.contents !== void 0;
        return identity.isCollection(this.contents) ? this.contents.hasIn(path) : false;
      }
      /**
       * Sets a value in this document. For `!!set`, `value` needs to be a
       * boolean to add/remove the item from the set.
       */
      set(key, value) {
        if (this.contents == null) {
          this.contents = Collection.collectionFromPath(this.schema, [key], value);
        } else if (assertCollection(this.contents)) {
          this.contents.set(key, value);
        }
      }
      /**
       * Sets a value in this document. For `!!set`, `value` needs to be a
       * boolean to add/remove the item from the set.
       */
      setIn(path, value) {
        if (Collection.isEmptyPath(path)) {
          this.contents = value;
        } else if (this.contents == null) {
          this.contents = Collection.collectionFromPath(this.schema, Array.from(path), value);
        } else if (assertCollection(this.contents)) {
          this.contents.setIn(path, value);
        }
      }
      /**
       * Change the YAML version and schema used by the document.
       * A `null` version disables support for directives, explicit tags, anchors, and aliases.
       * It also requires the `schema` option to be given as a `Schema` instance value.
       *
       * Overrides all previously set schema options.
       */
      setSchema(version, options = {}) {
        if (typeof version === "number")
          version = String(version);
        let opt;
        switch (version) {
          case "1.1":
            if (this.directives)
              this.directives.yaml.version = "1.1";
            else
              this.directives = new directives.Directives({ version: "1.1" });
            opt = { resolveKnownTags: false, schema: "yaml-1.1" };
            break;
          case "1.2":
          case "next":
            if (this.directives)
              this.directives.yaml.version = version;
            else
              this.directives = new directives.Directives({ version });
            opt = { resolveKnownTags: true, schema: "core" };
            break;
          case null:
            if (this.directives)
              delete this.directives;
            opt = null;
            break;
          default: {
            const sv = JSON.stringify(version);
            throw new Error(`Expected '1.1', '1.2' or null as first argument, but found: ${sv}`);
          }
        }
        if (options.schema instanceof Object)
          this.schema = options.schema;
        else if (opt)
          this.schema = new Schema.Schema(Object.assign(opt, options));
        else
          throw new Error(`With a null YAML version, the { schema: Schema } option is required`);
      }
      // json & jsonArg are only used from toJSON()
      toJS({ json, jsonArg, mapAsMap, maxAliasCount, onAnchor, reviver } = {}) {
        const ctx = {
          anchors: /* @__PURE__ */ new Map(),
          doc: this,
          keep: !json,
          mapAsMap: mapAsMap === true,
          mapKeyWarned: false,
          maxAliasCount: typeof maxAliasCount === "number" ? maxAliasCount : 100
        };
        const res = toJS.toJS(this.contents, jsonArg ?? "", ctx);
        if (typeof onAnchor === "function")
          for (const { count, res: res2 } of ctx.anchors.values())
            onAnchor(res2, count);
        return typeof reviver === "function" ? applyReviver.applyReviver(reviver, { "": res }, "", res) : res;
      }
      /**
       * A JSON representation of the document `contents`.
       *
       * @param jsonArg Used by `JSON.stringify` to indicate the array index or
       *   property name.
       */
      toJSON(jsonArg, onAnchor) {
        return this.toJS({ json: true, jsonArg, mapAsMap: false, onAnchor });
      }
      /** A YAML representation of the document. */
      toString(options = {}) {
        if (this.errors.length > 0)
          throw new Error("Document with errors cannot be stringified");
        if ("indent" in options && (!Number.isInteger(options.indent) || Number(options.indent) <= 0)) {
          const s = JSON.stringify(options.indent);
          throw new Error(`"indent" option must be a positive integer, not ${s}`);
        }
        return stringifyDocument.stringifyDocument(this, options);
      }
    };
    function assertCollection(contents) {
      if (identity.isCollection(contents))
        return true;
      throw new Error("Expected a YAML collection as document contents");
    }
    exports.Document = Document;
  }
});

// node_modules/yaml/dist/errors.js
var require_errors = __commonJS({
  "node_modules/yaml/dist/errors.js"(exports) {
    "use strict";
    var YAMLError = class extends Error {
      constructor(name, pos, code, message) {
        super();
        this.name = name;
        this.code = code;
        this.message = message;
        this.pos = pos;
      }
    };
    var YAMLParseError = class extends YAMLError {
      constructor(pos, code, message) {
        super("YAMLParseError", pos, code, message);
      }
    };
    var YAMLWarning = class extends YAMLError {
      constructor(pos, code, message) {
        super("YAMLWarning", pos, code, message);
      }
    };
    var prettifyError = (src, lc) => (error) => {
      if (error.pos[0] === -1)
        return;
      error.linePos = error.pos.map((pos) => lc.linePos(pos));
      const { line, col } = error.linePos[0];
      error.message += ` at line ${line}, column ${col}`;
      let ci = col - 1;
      let lineStr = src.substring(lc.lineStarts[line - 1], lc.lineStarts[line]).replace(/[\n\r]+$/, "");
      if (ci >= 60 && lineStr.length > 80) {
        const trimStart = Math.min(ci - 39, lineStr.length - 79);
        lineStr = "\u2026" + lineStr.substring(trimStart);
        ci -= trimStart - 1;
      }
      if (lineStr.length > 80)
        lineStr = lineStr.substring(0, 79) + "\u2026";
      if (line > 1 && /^ *$/.test(lineStr.substring(0, ci))) {
        let prev = src.substring(lc.lineStarts[line - 2], lc.lineStarts[line - 1]);
        if (prev.length > 80)
          prev = prev.substring(0, 79) + "\u2026\n";
        lineStr = prev + lineStr;
      }
      if (/[^ ]/.test(lineStr)) {
        let count = 1;
        const end = error.linePos[1];
        if (end?.line === line && end.col > col) {
          count = Math.max(1, Math.min(end.col - col, 80 - ci));
        }
        const pointer = " ".repeat(ci) + "^".repeat(count);
        error.message += `:

${lineStr}
${pointer}
`;
      }
    };
    exports.YAMLError = YAMLError;
    exports.YAMLParseError = YAMLParseError;
    exports.YAMLWarning = YAMLWarning;
    exports.prettifyError = prettifyError;
  }
});

// node_modules/yaml/dist/compose/resolve-props.js
var require_resolve_props = __commonJS({
  "node_modules/yaml/dist/compose/resolve-props.js"(exports) {
    "use strict";
    function resolveProps(tokens, { flow, indicator, next, offset, onError, parentIndent, startOnNewline }) {
      let spaceBefore = false;
      let atNewline = startOnNewline;
      let hasSpace = startOnNewline;
      let comment = "";
      let commentSep = "";
      let hasNewline = false;
      let reqSpace = false;
      let tab = null;
      let anchor = null;
      let tag = null;
      let newlineAfterProp = null;
      let comma = null;
      let found = null;
      let start = null;
      for (const token of tokens) {
        if (reqSpace) {
          if (token.type !== "space" && token.type !== "newline" && token.type !== "comma")
            onError(token.offset, "MISSING_CHAR", "Tags and anchors must be separated from the next token by white space");
          reqSpace = false;
        }
        if (tab) {
          if (atNewline && token.type !== "comment" && token.type !== "newline") {
            onError(tab, "TAB_AS_INDENT", "Tabs are not allowed as indentation");
          }
          tab = null;
        }
        switch (token.type) {
          case "space":
            if (!flow && (indicator !== "doc-start" || next?.type !== "flow-collection") && token.source.includes("	")) {
              tab = token;
            }
            hasSpace = true;
            break;
          case "comment": {
            if (!hasSpace)
              onError(token, "MISSING_CHAR", "Comments must be separated from other tokens by white space characters");
            const cb = token.source.substring(1) || " ";
            if (!comment)
              comment = cb;
            else
              comment += commentSep + cb;
            commentSep = "";
            atNewline = false;
            break;
          }
          case "newline":
            if (atNewline) {
              if (comment)
                comment += token.source;
              else if (!found || indicator !== "seq-item-ind")
                spaceBefore = true;
            } else
              commentSep += token.source;
            atNewline = true;
            hasNewline = true;
            if (anchor || tag)
              newlineAfterProp = token;
            hasSpace = true;
            break;
          case "anchor":
            if (anchor)
              onError(token, "MULTIPLE_ANCHORS", "A node can have at most one anchor");
            if (token.source.endsWith(":"))
              onError(token.offset + token.source.length - 1, "BAD_ALIAS", "Anchor ending in : is ambiguous", true);
            anchor = token;
            start ?? (start = token.offset);
            atNewline = false;
            hasSpace = false;
            reqSpace = true;
            break;
          case "tag": {
            if (tag)
              onError(token, "MULTIPLE_TAGS", "A node can have at most one tag");
            tag = token;
            start ?? (start = token.offset);
            atNewline = false;
            hasSpace = false;
            reqSpace = true;
            break;
          }
          case indicator:
            if (anchor || tag)
              onError(token, "BAD_PROP_ORDER", `Anchors and tags must be after the ${token.source} indicator`);
            if (found)
              onError(token, "UNEXPECTED_TOKEN", `Unexpected ${token.source} in ${flow ?? "collection"}`);
            found = token;
            atNewline = indicator === "seq-item-ind" || indicator === "explicit-key-ind";
            hasSpace = false;
            break;
          case "comma":
            if (flow) {
              if (comma)
                onError(token, "UNEXPECTED_TOKEN", `Unexpected , in ${flow}`);
              comma = token;
              atNewline = false;
              hasSpace = false;
              break;
            }
          // else fallthrough
          default:
            onError(token, "UNEXPECTED_TOKEN", `Unexpected ${token.type} token`);
            atNewline = false;
            hasSpace = false;
        }
      }
      const last = tokens[tokens.length - 1];
      const end = last ? last.offset + last.source.length : offset;
      if (reqSpace && next && next.type !== "space" && next.type !== "newline" && next.type !== "comma" && (next.type !== "scalar" || next.source !== "")) {
        onError(next.offset, "MISSING_CHAR", "Tags and anchors must be separated from the next token by white space");
      }
      if (tab && (atNewline && tab.indent <= parentIndent || next?.type === "block-map" || next?.type === "block-seq"))
        onError(tab, "TAB_AS_INDENT", "Tabs are not allowed as indentation");
      return {
        comma,
        found,
        spaceBefore,
        comment,
        hasNewline,
        anchor,
        tag,
        newlineAfterProp,
        end,
        start: start ?? end
      };
    }
    exports.resolveProps = resolveProps;
  }
});

// node_modules/yaml/dist/compose/util-contains-newline.js
var require_util_contains_newline = __commonJS({
  "node_modules/yaml/dist/compose/util-contains-newline.js"(exports) {
    "use strict";
    function containsNewline(key) {
      if (!key)
        return null;
      switch (key.type) {
        case "alias":
        case "scalar":
        case "double-quoted-scalar":
        case "single-quoted-scalar":
          if (key.source.includes("\n"))
            return true;
          if (key.end) {
            for (const st of key.end)
              if (st.type === "newline")
                return true;
          }
          return false;
        case "flow-collection":
          for (const it of key.items) {
            for (const st of it.start)
              if (st.type === "newline")
                return true;
            if (it.sep) {
              for (const st of it.sep)
                if (st.type === "newline")
                  return true;
            }
            if (containsNewline(it.key) || containsNewline(it.value))
              return true;
          }
          return false;
        default:
          return true;
      }
    }
    exports.containsNewline = containsNewline;
  }
});

// node_modules/yaml/dist/compose/util-flow-indent-check.js
var require_util_flow_indent_check = __commonJS({
  "node_modules/yaml/dist/compose/util-flow-indent-check.js"(exports) {
    "use strict";
    var utilContainsNewline = require_util_contains_newline();
    function flowIndentCheck(indent, fc, onError) {
      if (fc?.type === "flow-collection") {
        const end = fc.end[0];
        if (end.indent === indent && (end.source === "]" || end.source === "}") && utilContainsNewline.containsNewline(fc)) {
          const msg = "Flow end indicator should be more indented than parent";
          onError(end, "BAD_INDENT", msg, true);
        }
      }
    }
    exports.flowIndentCheck = flowIndentCheck;
  }
});

// node_modules/yaml/dist/compose/util-map-includes.js
var require_util_map_includes = __commonJS({
  "node_modules/yaml/dist/compose/util-map-includes.js"(exports) {
    "use strict";
    var identity = require_identity();
    function mapIncludes(ctx, items, search) {
      const { uniqueKeys } = ctx.options;
      if (uniqueKeys === false)
        return false;
      const isEqual = typeof uniqueKeys === "function" ? uniqueKeys : (a, b) => a === b || identity.isScalar(a) && identity.isScalar(b) && a.value === b.value;
      return items.some((pair) => isEqual(pair.key, search));
    }
    exports.mapIncludes = mapIncludes;
  }
});

// node_modules/yaml/dist/compose/resolve-block-map.js
var require_resolve_block_map = __commonJS({
  "node_modules/yaml/dist/compose/resolve-block-map.js"(exports) {
    "use strict";
    var Pair = require_Pair();
    var YAMLMap = require_YAMLMap();
    var resolveProps = require_resolve_props();
    var utilContainsNewline = require_util_contains_newline();
    var utilFlowIndentCheck = require_util_flow_indent_check();
    var utilMapIncludes = require_util_map_includes();
    var startColMsg = "All mapping items must start at the same column";
    function resolveBlockMap({ composeNode, composeEmptyNode }, ctx, bm, onError, tag) {
      const NodeClass = tag?.nodeClass ?? YAMLMap.YAMLMap;
      const map = new NodeClass(ctx.schema);
      if (ctx.atRoot)
        ctx.atRoot = false;
      let offset = bm.offset;
      let commentEnd = null;
      for (const collItem of bm.items) {
        const { start, key, sep, value } = collItem;
        const keyProps = resolveProps.resolveProps(start, {
          indicator: "explicit-key-ind",
          next: key ?? sep?.[0],
          offset,
          onError,
          parentIndent: bm.indent,
          startOnNewline: true
        });
        const implicitKey = !keyProps.found;
        if (implicitKey) {
          if (key) {
            if (key.type === "block-seq")
              onError(offset, "BLOCK_AS_IMPLICIT_KEY", "A block sequence may not be used as an implicit map key");
            else if ("indent" in key && key.indent !== bm.indent)
              onError(offset, "BAD_INDENT", startColMsg);
          }
          if (!keyProps.anchor && !keyProps.tag && !sep) {
            commentEnd = keyProps.end;
            if (keyProps.comment) {
              if (map.comment)
                map.comment += "\n" + keyProps.comment;
              else
                map.comment = keyProps.comment;
            }
            continue;
          }
          if (keyProps.newlineAfterProp || utilContainsNewline.containsNewline(key)) {
            onError(key ?? start[start.length - 1], "MULTILINE_IMPLICIT_KEY", "Implicit keys need to be on a single line");
          }
        } else if (keyProps.found?.indent !== bm.indent) {
          onError(offset, "BAD_INDENT", startColMsg);
        }
        ctx.atKey = true;
        const keyStart = keyProps.end;
        const keyNode = key ? composeNode(ctx, key, keyProps, onError) : composeEmptyNode(ctx, keyStart, start, null, keyProps, onError);
        if (ctx.schema.compat)
          utilFlowIndentCheck.flowIndentCheck(bm.indent, key, onError);
        ctx.atKey = false;
        if (utilMapIncludes.mapIncludes(ctx, map.items, keyNode))
          onError(keyStart, "DUPLICATE_KEY", "Map keys must be unique");
        const valueProps = resolveProps.resolveProps(sep ?? [], {
          indicator: "map-value-ind",
          next: value,
          offset: keyNode.range[2],
          onError,
          parentIndent: bm.indent,
          startOnNewline: !key || key.type === "block-scalar"
        });
        offset = valueProps.end;
        if (valueProps.found) {
          if (implicitKey) {
            if (value?.type === "block-map" && !valueProps.hasNewline)
              onError(offset, "BLOCK_AS_IMPLICIT_KEY", "Nested mappings are not allowed in compact mappings");
            if (ctx.options.strict && keyProps.start < valueProps.found.offset - 1024)
              onError(keyNode.range, "KEY_OVER_1024_CHARS", "The : indicator must be at most 1024 chars after the start of an implicit block mapping key");
          }
          const valueNode = value ? composeNode(ctx, value, valueProps, onError) : composeEmptyNode(ctx, offset, sep, null, valueProps, onError);
          if (ctx.schema.compat)
            utilFlowIndentCheck.flowIndentCheck(bm.indent, value, onError);
          offset = valueNode.range[2];
          const pair = new Pair.Pair(keyNode, valueNode);
          if (ctx.options.keepSourceTokens)
            pair.srcToken = collItem;
          map.items.push(pair);
        } else {
          if (implicitKey)
            onError(keyNode.range, "MISSING_CHAR", "Implicit map keys need to be followed by map values");
          if (valueProps.comment) {
            if (keyNode.comment)
              keyNode.comment += "\n" + valueProps.comment;
            else
              keyNode.comment = valueProps.comment;
          }
          const pair = new Pair.Pair(keyNode);
          if (ctx.options.keepSourceTokens)
            pair.srcToken = collItem;
          map.items.push(pair);
        }
      }
      if (commentEnd && commentEnd < offset)
        onError(commentEnd, "IMPOSSIBLE", "Map comment with trailing content");
      map.range = [bm.offset, offset, commentEnd ?? offset];
      return map;
    }
    exports.resolveBlockMap = resolveBlockMap;
  }
});

// node_modules/yaml/dist/compose/resolve-block-seq.js
var require_resolve_block_seq = __commonJS({
  "node_modules/yaml/dist/compose/resolve-block-seq.js"(exports) {
    "use strict";
    var YAMLSeq = require_YAMLSeq();
    var resolveProps = require_resolve_props();
    var utilFlowIndentCheck = require_util_flow_indent_check();
    function resolveBlockSeq({ composeNode, composeEmptyNode }, ctx, bs, onError, tag) {
      const NodeClass = tag?.nodeClass ?? YAMLSeq.YAMLSeq;
      const seq = new NodeClass(ctx.schema);
      if (ctx.atRoot)
        ctx.atRoot = false;
      if (ctx.atKey)
        ctx.atKey = false;
      let offset = bs.offset;
      let commentEnd = null;
      for (const { start, value } of bs.items) {
        const props = resolveProps.resolveProps(start, {
          indicator: "seq-item-ind",
          next: value,
          offset,
          onError,
          parentIndent: bs.indent,
          startOnNewline: true
        });
        if (!props.found) {
          if (props.anchor || props.tag || value) {
            if (value?.type === "block-seq")
              onError(props.end, "BAD_INDENT", "All sequence items must start at the same column");
            else
              onError(offset, "MISSING_CHAR", "Sequence item without - indicator");
          } else {
            commentEnd = props.end;
            if (props.comment)
              seq.comment = props.comment;
            continue;
          }
        }
        const node = value ? composeNode(ctx, value, props, onError) : composeEmptyNode(ctx, props.end, start, null, props, onError);
        if (ctx.schema.compat)
          utilFlowIndentCheck.flowIndentCheck(bs.indent, value, onError);
        offset = node.range[2];
        seq.items.push(node);
      }
      seq.range = [bs.offset, offset, commentEnd ?? offset];
      return seq;
    }
    exports.resolveBlockSeq = resolveBlockSeq;
  }
});

// node_modules/yaml/dist/compose/resolve-end.js
var require_resolve_end = __commonJS({
  "node_modules/yaml/dist/compose/resolve-end.js"(exports) {
    "use strict";
    function resolveEnd(end, offset, reqSpace, onError) {
      let comment = "";
      if (end) {
        let hasSpace = false;
        let sep = "";
        for (const token of end) {
          const { source, type } = token;
          switch (type) {
            case "space":
              hasSpace = true;
              break;
            case "comment": {
              if (reqSpace && !hasSpace)
                onError(token, "MISSING_CHAR", "Comments must be separated from other tokens by white space characters");
              const cb = source.substring(1) || " ";
              if (!comment)
                comment = cb;
              else
                comment += sep + cb;
              sep = "";
              break;
            }
            case "newline":
              if (comment)
                sep += source;
              hasSpace = true;
              break;
            default:
              onError(token, "UNEXPECTED_TOKEN", `Unexpected ${type} at node end`);
          }
          offset += source.length;
        }
      }
      return { comment, offset };
    }
    exports.resolveEnd = resolveEnd;
  }
});

// node_modules/yaml/dist/compose/resolve-flow-collection.js
var require_resolve_flow_collection = __commonJS({
  "node_modules/yaml/dist/compose/resolve-flow-collection.js"(exports) {
    "use strict";
    var identity = require_identity();
    var Pair = require_Pair();
    var YAMLMap = require_YAMLMap();
    var YAMLSeq = require_YAMLSeq();
    var resolveEnd = require_resolve_end();
    var resolveProps = require_resolve_props();
    var utilContainsNewline = require_util_contains_newline();
    var utilMapIncludes = require_util_map_includes();
    var blockMsg = "Block collections are not allowed within flow collections";
    var isBlock = (token) => token && (token.type === "block-map" || token.type === "block-seq");
    function resolveFlowCollection({ composeNode, composeEmptyNode }, ctx, fc, onError, tag) {
      const isMap = fc.start.source === "{";
      const fcName = isMap ? "flow map" : "flow sequence";
      const NodeClass = tag?.nodeClass ?? (isMap ? YAMLMap.YAMLMap : YAMLSeq.YAMLSeq);
      const coll = new NodeClass(ctx.schema);
      coll.flow = true;
      const atRoot = ctx.atRoot;
      if (atRoot)
        ctx.atRoot = false;
      if (ctx.atKey)
        ctx.atKey = false;
      let offset = fc.offset + fc.start.source.length;
      for (let i = 0; i < fc.items.length; ++i) {
        const collItem = fc.items[i];
        const { start, key, sep, value } = collItem;
        const props = resolveProps.resolveProps(start, {
          flow: fcName,
          indicator: "explicit-key-ind",
          next: key ?? sep?.[0],
          offset,
          onError,
          parentIndent: fc.indent,
          startOnNewline: false
        });
        if (!props.found) {
          if (!props.anchor && !props.tag && !sep && !value) {
            if (i === 0 && props.comma)
              onError(props.comma, "UNEXPECTED_TOKEN", `Unexpected , in ${fcName}`);
            else if (i < fc.items.length - 1)
              onError(props.start, "UNEXPECTED_TOKEN", `Unexpected empty item in ${fcName}`);
            if (props.comment) {
              if (coll.comment)
                coll.comment += "\n" + props.comment;
              else
                coll.comment = props.comment;
            }
            offset = props.end;
            continue;
          }
          if (!isMap && ctx.options.strict && utilContainsNewline.containsNewline(key))
            onError(
              key,
              // checked by containsNewline()
              "MULTILINE_IMPLICIT_KEY",
              "Implicit keys of flow sequence pairs need to be on a single line"
            );
        }
        if (i === 0) {
          if (props.comma)
            onError(props.comma, "UNEXPECTED_TOKEN", `Unexpected , in ${fcName}`);
        } else {
          if (!props.comma)
            onError(props.start, "MISSING_CHAR", `Missing , between ${fcName} items`);
          if (props.comment) {
            let prevItemComment = "";
            loop: for (const st of start) {
              switch (st.type) {
                case "comma":
                case "space":
                  break;
                case "comment":
                  prevItemComment = st.source.substring(1);
                  break loop;
                default:
                  break loop;
              }
            }
            if (prevItemComment) {
              let prev = coll.items[coll.items.length - 1];
              if (identity.isPair(prev))
                prev = prev.value ?? prev.key;
              if (prev.comment)
                prev.comment += "\n" + prevItemComment;
              else
                prev.comment = prevItemComment;
              props.comment = props.comment.substring(prevItemComment.length + 1);
            }
          }
        }
        if (!isMap && !sep && !props.found) {
          const valueNode = value ? composeNode(ctx, value, props, onError) : composeEmptyNode(ctx, props.end, sep, null, props, onError);
          coll.items.push(valueNode);
          offset = valueNode.range[2];
          if (isBlock(value))
            onError(valueNode.range, "BLOCK_IN_FLOW", blockMsg);
        } else {
          ctx.atKey = true;
          const keyStart = props.end;
          const keyNode = key ? composeNode(ctx, key, props, onError) : composeEmptyNode(ctx, keyStart, start, null, props, onError);
          if (isBlock(key))
            onError(keyNode.range, "BLOCK_IN_FLOW", blockMsg);
          ctx.atKey = false;
          const valueProps = resolveProps.resolveProps(sep ?? [], {
            flow: fcName,
            indicator: "map-value-ind",
            next: value,
            offset: keyNode.range[2],
            onError,
            parentIndent: fc.indent,
            startOnNewline: false
          });
          if (valueProps.found) {
            if (!isMap && !props.found && ctx.options.strict) {
              if (sep)
                for (const st of sep) {
                  if (st === valueProps.found)
                    break;
                  if (st.type === "newline") {
                    onError(st, "MULTILINE_IMPLICIT_KEY", "Implicit keys of flow sequence pairs need to be on a single line");
                    break;
                  }
                }
              if (props.start < valueProps.found.offset - 1024)
                onError(valueProps.found, "KEY_OVER_1024_CHARS", "The : indicator must be at most 1024 chars after the start of an implicit flow sequence key");
            }
          } else if (value) {
            if ("source" in value && value.source?.[0] === ":")
              onError(value, "MISSING_CHAR", `Missing space after : in ${fcName}`);
            else
              onError(valueProps.start, "MISSING_CHAR", `Missing , or : between ${fcName} items`);
          }
          const valueNode = value ? composeNode(ctx, value, valueProps, onError) : valueProps.found ? composeEmptyNode(ctx, valueProps.end, sep, null, valueProps, onError) : null;
          if (valueNode) {
            if (isBlock(value))
              onError(valueNode.range, "BLOCK_IN_FLOW", blockMsg);
          } else if (valueProps.comment) {
            if (keyNode.comment)
              keyNode.comment += "\n" + valueProps.comment;
            else
              keyNode.comment = valueProps.comment;
          }
          const pair = new Pair.Pair(keyNode, valueNode);
          if (ctx.options.keepSourceTokens)
            pair.srcToken = collItem;
          if (isMap) {
            const map = coll;
            if (utilMapIncludes.mapIncludes(ctx, map.items, keyNode))
              onError(keyStart, "DUPLICATE_KEY", "Map keys must be unique");
            map.items.push(pair);
          } else {
            const map = new YAMLMap.YAMLMap(ctx.schema);
            map.flow = true;
            map.items.push(pair);
            const endRange = (valueNode ?? keyNode).range;
            map.range = [keyNode.range[0], endRange[1], endRange[2]];
            coll.items.push(map);
          }
          offset = valueNode ? valueNode.range[2] : valueProps.end;
        }
      }
      const expectedEnd = isMap ? "}" : "]";
      const [ce, ...ee] = fc.end;
      let cePos = offset;
      if (ce?.source === expectedEnd)
        cePos = ce.offset + ce.source.length;
      else {
        const name = fcName[0].toUpperCase() + fcName.substring(1);
        const msg = atRoot ? `${name} must end with a ${expectedEnd}` : `${name} in block collection must be sufficiently indented and end with a ${expectedEnd}`;
        onError(offset, atRoot ? "MISSING_CHAR" : "BAD_INDENT", msg);
        if (ce && ce.source.length !== 1)
          ee.unshift(ce);
      }
      if (ee.length > 0) {
        const end = resolveEnd.resolveEnd(ee, cePos, ctx.options.strict, onError);
        if (end.comment) {
          if (coll.comment)
            coll.comment += "\n" + end.comment;
          else
            coll.comment = end.comment;
        }
        coll.range = [fc.offset, cePos, end.offset];
      } else {
        coll.range = [fc.offset, cePos, cePos];
      }
      return coll;
    }
    exports.resolveFlowCollection = resolveFlowCollection;
  }
});

// node_modules/yaml/dist/compose/compose-collection.js
var require_compose_collection = __commonJS({
  "node_modules/yaml/dist/compose/compose-collection.js"(exports) {
    "use strict";
    var identity = require_identity();
    var Scalar = require_Scalar();
    var YAMLMap = require_YAMLMap();
    var YAMLSeq = require_YAMLSeq();
    var resolveBlockMap = require_resolve_block_map();
    var resolveBlockSeq = require_resolve_block_seq();
    var resolveFlowCollection = require_resolve_flow_collection();
    function resolveCollection(CN, ctx, token, onError, tagName, tag) {
      const coll = token.type === "block-map" ? resolveBlockMap.resolveBlockMap(CN, ctx, token, onError, tag) : token.type === "block-seq" ? resolveBlockSeq.resolveBlockSeq(CN, ctx, token, onError, tag) : resolveFlowCollection.resolveFlowCollection(CN, ctx, token, onError, tag);
      const Coll = coll.constructor;
      if (tagName === "!" || tagName === Coll.tagName) {
        coll.tag = Coll.tagName;
        return coll;
      }
      if (tagName)
        coll.tag = tagName;
      return coll;
    }
    function composeCollection(CN, ctx, token, props, onError) {
      const tagToken = props.tag;
      const tagName = !tagToken ? null : ctx.directives.tagName(tagToken.source, (msg) => onError(tagToken, "TAG_RESOLVE_FAILED", msg));
      if (token.type === "block-seq") {
        const { anchor, newlineAfterProp: nl } = props;
        const lastProp = anchor && tagToken ? anchor.offset > tagToken.offset ? anchor : tagToken : anchor ?? tagToken;
        if (lastProp && (!nl || nl.offset < lastProp.offset)) {
          const message = "Missing newline after block sequence props";
          onError(lastProp, "MISSING_CHAR", message);
        }
      }
      const expType = token.type === "block-map" ? "map" : token.type === "block-seq" ? "seq" : token.start.source === "{" ? "map" : "seq";
      if (!tagToken || !tagName || tagName === "!" || tagName === YAMLMap.YAMLMap.tagName && expType === "map" || tagName === YAMLSeq.YAMLSeq.tagName && expType === "seq") {
        return resolveCollection(CN, ctx, token, onError, tagName);
      }
      let tag = ctx.schema.tags.find((t) => t.tag === tagName && t.collection === expType);
      if (!tag) {
        const kt = ctx.schema.knownTags[tagName];
        if (kt?.collection === expType) {
          ctx.schema.tags.push(Object.assign({}, kt, { default: false }));
          tag = kt;
        } else {
          if (kt) {
            onError(tagToken, "BAD_COLLECTION_TYPE", `${kt.tag} used for ${expType} collection, but expects ${kt.collection ?? "scalar"}`, true);
          } else {
            onError(tagToken, "TAG_RESOLVE_FAILED", `Unresolved tag: ${tagName}`, true);
          }
          return resolveCollection(CN, ctx, token, onError, tagName);
        }
      }
      const coll = resolveCollection(CN, ctx, token, onError, tagName, tag);
      const res = tag.resolve?.(coll, (msg) => onError(tagToken, "TAG_RESOLVE_FAILED", msg), ctx.options) ?? coll;
      const node = identity.isNode(res) ? res : new Scalar.Scalar(res);
      node.range = coll.range;
      node.tag = tagName;
      if (tag?.format)
        node.format = tag.format;
      return node;
    }
    exports.composeCollection = composeCollection;
  }
});

// node_modules/yaml/dist/compose/resolve-block-scalar.js
var require_resolve_block_scalar = __commonJS({
  "node_modules/yaml/dist/compose/resolve-block-scalar.js"(exports) {
    "use strict";
    var Scalar = require_Scalar();
    function resolveBlockScalar(ctx, scalar, onError) {
      const start = scalar.offset;
      const header = parseBlockScalarHeader(scalar, ctx.options.strict, onError);
      if (!header)
        return { value: "", type: null, comment: "", range: [start, start, start] };
      const type = header.mode === ">" ? Scalar.Scalar.BLOCK_FOLDED : Scalar.Scalar.BLOCK_LITERAL;
      const lines = scalar.source ? splitLines(scalar.source) : [];
      let chompStart = lines.length;
      for (let i = lines.length - 1; i >= 0; --i) {
        const content = lines[i][1];
        if (content === "" || content === "\r")
          chompStart = i;
        else
          break;
      }
      if (chompStart === 0) {
        const value2 = header.chomp === "+" && lines.length > 0 ? "\n".repeat(Math.max(1, lines.length - 1)) : "";
        let end2 = start + header.length;
        if (scalar.source)
          end2 += scalar.source.length;
        return { value: value2, type, comment: header.comment, range: [start, end2, end2] };
      }
      let trimIndent = scalar.indent + header.indent;
      let offset = scalar.offset + header.length;
      let contentStart = 0;
      for (let i = 0; i < chompStart; ++i) {
        const [indent, content] = lines[i];
        if (content === "" || content === "\r") {
          if (header.indent === 0 && indent.length > trimIndent)
            trimIndent = indent.length;
        } else {
          if (indent.length < trimIndent) {
            const message = "Block scalars with more-indented leading empty lines must use an explicit indentation indicator";
            onError(offset + indent.length, "MISSING_CHAR", message);
          }
          if (header.indent === 0)
            trimIndent = indent.length;
          contentStart = i;
          if (trimIndent === 0 && !ctx.atRoot) {
            const message = "Block scalar values in collections must be indented";
            onError(offset, "BAD_INDENT", message);
          }
          break;
        }
        offset += indent.length + content.length + 1;
      }
      for (let i = lines.length - 1; i >= chompStart; --i) {
        if (lines[i][0].length > trimIndent)
          chompStart = i + 1;
      }
      let value = "";
      let sep = "";
      let prevMoreIndented = false;
      for (let i = 0; i < contentStart; ++i)
        value += lines[i][0].slice(trimIndent) + "\n";
      for (let i = contentStart; i < chompStart; ++i) {
        let [indent, content] = lines[i];
        offset += indent.length + content.length + 1;
        const crlf = content[content.length - 1] === "\r";
        if (crlf)
          content = content.slice(0, -1);
        if (content && indent.length < trimIndent) {
          const src = header.indent ? "explicit indentation indicator" : "first line";
          const message = `Block scalar lines must not be less indented than their ${src}`;
          onError(offset - content.length - (crlf ? 2 : 1), "BAD_INDENT", message);
          indent = "";
        }
        if (type === Scalar.Scalar.BLOCK_LITERAL) {
          value += sep + indent.slice(trimIndent) + content;
          sep = "\n";
        } else if (indent.length > trimIndent || content[0] === "	") {
          if (sep === " ")
            sep = "\n";
          else if (!prevMoreIndented && sep === "\n")
            sep = "\n\n";
          value += sep + indent.slice(trimIndent) + content;
          sep = "\n";
          prevMoreIndented = true;
        } else if (content === "") {
          if (sep === "\n")
            value += "\n";
          else
            sep = "\n";
        } else {
          value += sep + content;
          sep = " ";
          prevMoreIndented = false;
        }
      }
      switch (header.chomp) {
        case "-":
          break;
        case "+":
          for (let i = chompStart; i < lines.length; ++i)
            value += "\n" + lines[i][0].slice(trimIndent);
          if (value[value.length - 1] !== "\n")
            value += "\n";
          break;
        default:
          value += "\n";
      }
      const end = start + header.length + scalar.source.length;
      return { value, type, comment: header.comment, range: [start, end, end] };
    }
    function parseBlockScalarHeader({ offset, props }, strict, onError) {
      if (props[0].type !== "block-scalar-header") {
        onError(props[0], "IMPOSSIBLE", "Block scalar header not found");
        return null;
      }
      const { source } = props[0];
      const mode = source[0];
      let indent = 0;
      let chomp = "";
      let error = -1;
      for (let i = 1; i < source.length; ++i) {
        const ch = source[i];
        if (!chomp && (ch === "-" || ch === "+"))
          chomp = ch;
        else {
          const n = Number(ch);
          if (!indent && n)
            indent = n;
          else if (error === -1)
            error = offset + i;
        }
      }
      if (error !== -1)
        onError(error, "UNEXPECTED_TOKEN", `Block scalar header includes extra characters: ${source}`);
      let hasSpace = false;
      let comment = "";
      let length = source.length;
      for (let i = 1; i < props.length; ++i) {
        const token = props[i];
        switch (token.type) {
          case "space":
            hasSpace = true;
          // fallthrough
          case "newline":
            length += token.source.length;
            break;
          case "comment":
            if (strict && !hasSpace) {
              const message = "Comments must be separated from other tokens by white space characters";
              onError(token, "MISSING_CHAR", message);
            }
            length += token.source.length;
            comment = token.source.substring(1);
            break;
          case "error":
            onError(token, "UNEXPECTED_TOKEN", token.message);
            length += token.source.length;
            break;
          /* istanbul ignore next should not happen */
          default: {
            const message = `Unexpected token in block scalar header: ${token.type}`;
            onError(token, "UNEXPECTED_TOKEN", message);
            const ts = token.source;
            if (ts && typeof ts === "string")
              length += ts.length;
          }
        }
      }
      return { mode, indent, chomp, comment, length };
    }
    function splitLines(source) {
      const split = source.split(/\n( *)/);
      const first = split[0];
      const m = first.match(/^( *)/);
      const line0 = m?.[1] ? [m[1], first.slice(m[1].length)] : ["", first];
      const lines = [line0];
      for (let i = 1; i < split.length; i += 2)
        lines.push([split[i], split[i + 1]]);
      return lines;
    }
    exports.resolveBlockScalar = resolveBlockScalar;
  }
});

// node_modules/yaml/dist/compose/resolve-flow-scalar.js
var require_resolve_flow_scalar = __commonJS({
  "node_modules/yaml/dist/compose/resolve-flow-scalar.js"(exports) {
    "use strict";
    var Scalar = require_Scalar();
    var resolveEnd = require_resolve_end();
    function resolveFlowScalar(scalar, strict, onError) {
      const { offset, type, source, end } = scalar;
      let _type;
      let value;
      const _onError = (rel, code, msg) => onError(offset + rel, code, msg);
      switch (type) {
        case "scalar":
          _type = Scalar.Scalar.PLAIN;
          value = plainValue(source, _onError);
          break;
        case "single-quoted-scalar":
          _type = Scalar.Scalar.QUOTE_SINGLE;
          value = singleQuotedValue(source, _onError);
          break;
        case "double-quoted-scalar":
          _type = Scalar.Scalar.QUOTE_DOUBLE;
          value = doubleQuotedValue(source, _onError);
          break;
        /* istanbul ignore next should not happen */
        default:
          onError(scalar, "UNEXPECTED_TOKEN", `Expected a flow scalar value, but found: ${type}`);
          return {
            value: "",
            type: null,
            comment: "",
            range: [offset, offset + source.length, offset + source.length]
          };
      }
      const valueEnd = offset + source.length;
      const re = resolveEnd.resolveEnd(end, valueEnd, strict, onError);
      return {
        value,
        type: _type,
        comment: re.comment,
        range: [offset, valueEnd, re.offset]
      };
    }
    function plainValue(source, onError) {
      let badChar = "";
      switch (source[0]) {
        /* istanbul ignore next should not happen */
        case "	":
          badChar = "a tab character";
          break;
        case ",":
          badChar = "flow indicator character ,";
          break;
        case "%":
          badChar = "directive indicator character %";
          break;
        case "|":
        case ">": {
          badChar = `block scalar indicator ${source[0]}`;
          break;
        }
        case "@":
        case "`": {
          badChar = `reserved character ${source[0]}`;
          break;
        }
      }
      if (badChar)
        onError(0, "BAD_SCALAR_START", `Plain value cannot start with ${badChar}`);
      return foldLines(source);
    }
    function singleQuotedValue(source, onError) {
      if (source[source.length - 1] !== "'" || source.length === 1)
        onError(source.length, "MISSING_CHAR", "Missing closing 'quote");
      return foldLines(source.slice(1, -1)).replace(/''/g, "'");
    }
    function foldLines(source) {
      let first, line;
      try {
        first = new RegExp("(.*?)(?<![ 	])[ 	]*\r?\n", "sy");
        line = new RegExp("[ 	]*(.*?)(?:(?<![ 	])[ 	]*)?\r?\n", "sy");
      } catch {
        first = /(.*?)[ \t]*\r?\n/sy;
        line = /[ \t]*(.*?)[ \t]*\r?\n/sy;
      }
      let match = first.exec(source);
      if (!match)
        return source;
      let res = match[1];
      let sep = " ";
      let pos = first.lastIndex;
      line.lastIndex = pos;
      while (match = line.exec(source)) {
        if (match[1] === "") {
          if (sep === "\n")
            res += sep;
          else
            sep = "\n";
        } else {
          res += sep + match[1];
          sep = " ";
        }
        pos = line.lastIndex;
      }
      const last = /[ \t]*(.*)/sy;
      last.lastIndex = pos;
      match = last.exec(source);
      return res + sep + (match?.[1] ?? "");
    }
    function doubleQuotedValue(source, onError) {
      let res = "";
      for (let i = 1; i < source.length - 1; ++i) {
        const ch = source[i];
        if (ch === "\r" && source[i + 1] === "\n")
          continue;
        if (ch === "\n") {
          const { fold, offset } = foldNewline(source, i);
          res += fold;
          i = offset;
        } else if (ch === "\\") {
          let next = source[++i];
          const cc = escapeCodes[next];
          if (cc)
            res += cc;
          else if (next === "\n") {
            next = source[i + 1];
            while (next === " " || next === "	")
              next = source[++i + 1];
          } else if (next === "\r" && source[i + 1] === "\n") {
            next = source[++i + 1];
            while (next === " " || next === "	")
              next = source[++i + 1];
          } else if (next === "x" || next === "u" || next === "U") {
            const length = next === "x" ? 2 : next === "u" ? 4 : 8;
            res += parseCharCode(source, i + 1, length, onError);
            i += length;
          } else {
            const raw = source.substr(i - 1, 2);
            onError(i - 1, "BAD_DQ_ESCAPE", `Invalid escape sequence ${raw}`);
            res += raw;
          }
        } else if (ch === " " || ch === "	") {
          const wsStart = i;
          let next = source[i + 1];
          while (next === " " || next === "	")
            next = source[++i + 1];
          if (next !== "\n" && !(next === "\r" && source[i + 2] === "\n"))
            res += i > wsStart ? source.slice(wsStart, i + 1) : ch;
        } else {
          res += ch;
        }
      }
      if (source[source.length - 1] !== '"' || source.length === 1)
        onError(source.length, "MISSING_CHAR", 'Missing closing "quote');
      return res;
    }
    function foldNewline(source, offset) {
      let fold = "";
      let ch = source[offset + 1];
      while (ch === " " || ch === "	" || ch === "\n" || ch === "\r") {
        if (ch === "\r" && source[offset + 2] !== "\n")
          break;
        if (ch === "\n")
          fold += "\n";
        offset += 1;
        ch = source[offset + 1];
      }
      if (!fold)
        fold = " ";
      return { fold, offset };
    }
    var escapeCodes = {
      "0": "\0",
      // null character
      a: "\x07",
      // bell character
      b: "\b",
      // backspace
      e: "\x1B",
      // escape character
      f: "\f",
      // form feed
      n: "\n",
      // line feed
      r: "\r",
      // carriage return
      t: "	",
      // horizontal tab
      v: "\v",
      // vertical tab
      N: "\x85",
      // Unicode next line
      _: "\xA0",
      // Unicode non-breaking space
      L: "\u2028",
      // Unicode line separator
      P: "\u2029",
      // Unicode paragraph separator
      " ": " ",
      '"': '"',
      "/": "/",
      "\\": "\\",
      "	": "	"
    };
    function parseCharCode(source, offset, length, onError) {
      const cc = source.substr(offset, length);
      const ok = cc.length === length && /^[0-9a-fA-F]+$/.test(cc);
      const code = ok ? parseInt(cc, 16) : NaN;
      try {
        return String.fromCodePoint(code);
      } catch {
        const raw = source.substr(offset - 2, length + 2);
        onError(offset - 2, "BAD_DQ_ESCAPE", `Invalid escape sequence ${raw}`);
        return raw;
      }
    }
    exports.resolveFlowScalar = resolveFlowScalar;
  }
});

// node_modules/yaml/dist/compose/compose-scalar.js
var require_compose_scalar = __commonJS({
  "node_modules/yaml/dist/compose/compose-scalar.js"(exports) {
    "use strict";
    var identity = require_identity();
    var Scalar = require_Scalar();
    var resolveBlockScalar = require_resolve_block_scalar();
    var resolveFlowScalar = require_resolve_flow_scalar();
    function composeScalar(ctx, token, tagToken, onError) {
      const { value, type, comment, range } = token.type === "block-scalar" ? resolveBlockScalar.resolveBlockScalar(ctx, token, onError) : resolveFlowScalar.resolveFlowScalar(token, ctx.options.strict, onError);
      const tagName = tagToken ? ctx.directives.tagName(tagToken.source, (msg) => onError(tagToken, "TAG_RESOLVE_FAILED", msg)) : null;
      let tag;
      if (ctx.options.stringKeys && ctx.atKey) {
        tag = ctx.schema[identity.SCALAR];
      } else if (tagName)
        tag = findScalarTagByName(ctx.schema, value, tagName, tagToken, onError);
      else if (token.type === "scalar")
        tag = findScalarTagByTest(ctx, value, token, onError);
      else
        tag = ctx.schema[identity.SCALAR];
      let scalar;
      try {
        const res = tag.resolve(value, (msg) => onError(tagToken ?? token, "TAG_RESOLVE_FAILED", msg), ctx.options);
        scalar = identity.isScalar(res) ? res : new Scalar.Scalar(res);
      } catch (error) {
        const msg = error instanceof Error ? error.message : String(error);
        onError(tagToken ?? token, "TAG_RESOLVE_FAILED", msg);
        scalar = new Scalar.Scalar(value);
      }
      scalar.range = range;
      scalar.source = value;
      if (type)
        scalar.type = type;
      if (tagName)
        scalar.tag = tagName;
      if (tag.format)
        scalar.format = tag.format;
      if (comment)
        scalar.comment = comment;
      return scalar;
    }
    function findScalarTagByName(schema, value, tagName, tagToken, onError) {
      if (tagName === "!")
        return schema[identity.SCALAR];
      const matchWithTest = [];
      for (const tag of schema.tags) {
        if (!tag.collection && tag.tag === tagName) {
          if (tag.default && tag.test)
            matchWithTest.push(tag);
          else
            return tag;
        }
      }
      for (const tag of matchWithTest)
        if (tag.test?.test(value))
          return tag;
      const kt = schema.knownTags[tagName];
      if (kt && !kt.collection) {
        schema.tags.push(Object.assign({}, kt, { default: false, test: void 0 }));
        return kt;
      }
      onError(tagToken, "TAG_RESOLVE_FAILED", `Unresolved tag: ${tagName}`, tagName !== "tag:yaml.org,2002:str");
      return schema[identity.SCALAR];
    }
    function findScalarTagByTest({ atKey, directives, schema }, value, token, onError) {
      const tag = schema.tags.find((tag2) => (tag2.default === true || atKey && tag2.default === "key") && tag2.test?.test(value)) || schema[identity.SCALAR];
      if (schema.compat) {
        const compat = schema.compat.find((tag2) => tag2.default && tag2.test?.test(value)) ?? schema[identity.SCALAR];
        if (tag.tag !== compat.tag) {
          const ts = directives.tagString(tag.tag);
          const cs = directives.tagString(compat.tag);
          const msg = `Value may be parsed as either ${ts} or ${cs}`;
          onError(token, "TAG_RESOLVE_FAILED", msg, true);
        }
      }
      return tag;
    }
    exports.composeScalar = composeScalar;
  }
});

// node_modules/yaml/dist/compose/util-empty-scalar-position.js
var require_util_empty_scalar_position = __commonJS({
  "node_modules/yaml/dist/compose/util-empty-scalar-position.js"(exports) {
    "use strict";
    function emptyScalarPosition(offset, before, pos) {
      if (before) {
        pos ?? (pos = before.length);
        for (let i = pos - 1; i >= 0; --i) {
          let st = before[i];
          switch (st.type) {
            case "space":
            case "comment":
            case "newline":
              offset -= st.source.length;
              continue;
          }
          st = before[++i];
          while (st?.type === "space") {
            offset += st.source.length;
            st = before[++i];
          }
          break;
        }
      }
      return offset;
    }
    exports.emptyScalarPosition = emptyScalarPosition;
  }
});

// node_modules/yaml/dist/compose/compose-node.js
var require_compose_node = __commonJS({
  "node_modules/yaml/dist/compose/compose-node.js"(exports) {
    "use strict";
    var Alias = require_Alias();
    var identity = require_identity();
    var composeCollection = require_compose_collection();
    var composeScalar = require_compose_scalar();
    var resolveEnd = require_resolve_end();
    var utilEmptyScalarPosition = require_util_empty_scalar_position();
    var CN = { composeNode, composeEmptyNode };
    function composeNode(ctx, token, props, onError) {
      const atKey = ctx.atKey;
      const { spaceBefore, comment, anchor, tag } = props;
      let node;
      let isSrcToken = true;
      switch (token.type) {
        case "alias":
          node = composeAlias(ctx, token, onError);
          if (anchor || tag)
            onError(token, "ALIAS_PROPS", "An alias node must not specify any properties");
          break;
        case "scalar":
        case "single-quoted-scalar":
        case "double-quoted-scalar":
        case "block-scalar":
          node = composeScalar.composeScalar(ctx, token, tag, onError);
          if (anchor)
            node.anchor = anchor.source.substring(1);
          break;
        case "block-map":
        case "block-seq":
        case "flow-collection":
          try {
            node = composeCollection.composeCollection(CN, ctx, token, props, onError);
            if (anchor)
              node.anchor = anchor.source.substring(1);
          } catch (error) {
            const message = error instanceof Error ? error.message : String(error);
            onError(token, "RESOURCE_EXHAUSTION", message);
          }
          break;
        default: {
          const message = token.type === "error" ? token.message : `Unsupported token (type: ${token.type})`;
          onError(token, "UNEXPECTED_TOKEN", message);
          isSrcToken = false;
        }
      }
      node ?? (node = composeEmptyNode(ctx, token.offset, void 0, null, props, onError));
      if (anchor && node.anchor === "")
        onError(anchor, "BAD_ALIAS", "Anchor cannot be an empty string");
      if (atKey && ctx.options.stringKeys && (!identity.isScalar(node) || typeof node.value !== "string" || node.tag && node.tag !== "tag:yaml.org,2002:str")) {
        const msg = "With stringKeys, all keys must be strings";
        onError(tag ?? token, "NON_STRING_KEY", msg);
      }
      if (spaceBefore)
        node.spaceBefore = true;
      if (comment) {
        if (token.type === "scalar" && token.source === "")
          node.comment = comment;
        else
          node.commentBefore = comment;
      }
      if (ctx.options.keepSourceTokens && isSrcToken)
        node.srcToken = token;
      return node;
    }
    function composeEmptyNode(ctx, offset, before, pos, { spaceBefore, comment, anchor, tag, end }, onError) {
      const token = {
        type: "scalar",
        offset: utilEmptyScalarPosition.emptyScalarPosition(offset, before, pos),
        indent: -1,
        source: ""
      };
      const node = composeScalar.composeScalar(ctx, token, tag, onError);
      if (anchor) {
        node.anchor = anchor.source.substring(1);
        if (node.anchor === "")
          onError(anchor, "BAD_ALIAS", "Anchor cannot be an empty string");
      }
      if (spaceBefore)
        node.spaceBefore = true;
      if (comment) {
        node.comment = comment;
        node.range[2] = end;
      }
      return node;
    }
    function composeAlias({ options }, { offset, source, end }, onError) {
      const alias = new Alias.Alias(source.substring(1));
      if (alias.source === "")
        onError(offset, "BAD_ALIAS", "Alias cannot be an empty string");
      if (alias.source.endsWith(":"))
        onError(offset + source.length - 1, "BAD_ALIAS", "Alias ending in : is ambiguous", true);
      const valueEnd = offset + source.length;
      const re = resolveEnd.resolveEnd(end, valueEnd, options.strict, onError);
      alias.range = [offset, valueEnd, re.offset];
      if (re.comment)
        alias.comment = re.comment;
      return alias;
    }
    exports.composeEmptyNode = composeEmptyNode;
    exports.composeNode = composeNode;
  }
});

// node_modules/yaml/dist/compose/compose-doc.js
var require_compose_doc = __commonJS({
  "node_modules/yaml/dist/compose/compose-doc.js"(exports) {
    "use strict";
    var Document = require_Document();
    var composeNode = require_compose_node();
    var resolveEnd = require_resolve_end();
    var resolveProps = require_resolve_props();
    function composeDoc(options, directives, { offset, start, value, end }, onError) {
      const opts = Object.assign({ _directives: directives }, options);
      const doc = new Document.Document(void 0, opts);
      const ctx = {
        atKey: false,
        atRoot: true,
        directives: doc.directives,
        options: doc.options,
        schema: doc.schema
      };
      const props = resolveProps.resolveProps(start, {
        indicator: "doc-start",
        next: value ?? end?.[0],
        offset,
        onError,
        parentIndent: 0,
        startOnNewline: true
      });
      if (props.found) {
        doc.directives.docStart = true;
        if (value && (value.type === "block-map" || value.type === "block-seq") && !props.hasNewline)
          onError(props.end, "MISSING_CHAR", "Block collection cannot start on same line with directives-end marker");
      }
      doc.contents = value ? composeNode.composeNode(ctx, value, props, onError) : composeNode.composeEmptyNode(ctx, props.end, start, null, props, onError);
      const contentEnd = doc.contents.range[2];
      const re = resolveEnd.resolveEnd(end, contentEnd, false, onError);
      if (re.comment)
        doc.comment = re.comment;
      doc.range = [offset, contentEnd, re.offset];
      return doc;
    }
    exports.composeDoc = composeDoc;
  }
});

// node_modules/yaml/dist/compose/composer.js
var require_composer = __commonJS({
  "node_modules/yaml/dist/compose/composer.js"(exports) {
    "use strict";
    var node_process = __require("process");
    var directives = require_directives();
    var Document = require_Document();
    var errors = require_errors();
    var identity = require_identity();
    var composeDoc = require_compose_doc();
    var resolveEnd = require_resolve_end();
    function getErrorPos(src) {
      if (typeof src === "number")
        return [src, src + 1];
      if (Array.isArray(src))
        return src.length === 2 ? src : [src[0], src[1]];
      const { offset, source } = src;
      return [offset, offset + (typeof source === "string" ? source.length : 1)];
    }
    function parsePrelude(prelude) {
      let comment = "";
      let atComment = false;
      let afterEmptyLine = false;
      for (let i = 0; i < prelude.length; ++i) {
        const source = prelude[i];
        switch (source[0]) {
          case "#":
            comment += (comment === "" ? "" : afterEmptyLine ? "\n\n" : "\n") + (source.substring(1) || " ");
            atComment = true;
            afterEmptyLine = false;
            break;
          case "%":
            if (prelude[i + 1]?.[0] !== "#")
              i += 1;
            atComment = false;
            break;
          default:
            if (!atComment)
              afterEmptyLine = true;
            atComment = false;
        }
      }
      return { comment, afterEmptyLine };
    }
    var Composer = class {
      constructor(options = {}) {
        this.doc = null;
        this.atDirectives = false;
        this.prelude = [];
        this.errors = [];
        this.warnings = [];
        this.onError = (source, code, message, warning) => {
          const pos = getErrorPos(source);
          if (warning)
            this.warnings.push(new errors.YAMLWarning(pos, code, message));
          else
            this.errors.push(new errors.YAMLParseError(pos, code, message));
        };
        this.directives = new directives.Directives({ version: options.version || "1.2" });
        this.options = options;
      }
      decorate(doc, afterDoc) {
        const { comment, afterEmptyLine } = parsePrelude(this.prelude);
        if (comment) {
          const dc = doc.contents;
          if (afterDoc) {
            doc.comment = doc.comment ? `${doc.comment}
${comment}` : comment;
          } else if (afterEmptyLine || doc.directives.docStart || !dc) {
            doc.commentBefore = comment;
          } else if (identity.isCollection(dc) && !dc.flow && dc.items.length > 0) {
            let it = dc.items[0];
            if (identity.isPair(it))
              it = it.key;
            const cb = it.commentBefore;
            it.commentBefore = cb ? `${comment}
${cb}` : comment;
          } else {
            const cb = dc.commentBefore;
            dc.commentBefore = cb ? `${comment}
${cb}` : comment;
          }
        }
        if (afterDoc) {
          for (let i = 0; i < this.errors.length; ++i)
            doc.errors.push(this.errors[i]);
          for (let i = 0; i < this.warnings.length; ++i)
            doc.warnings.push(this.warnings[i]);
        } else {
          doc.errors = this.errors;
          doc.warnings = this.warnings;
        }
        this.prelude = [];
        this.errors = [];
        this.warnings = [];
      }
      /**
       * Current stream status information.
       *
       * Mostly useful at the end of input for an empty stream.
       */
      streamInfo() {
        return {
          comment: parsePrelude(this.prelude).comment,
          directives: this.directives,
          errors: this.errors,
          warnings: this.warnings
        };
      }
      /**
       * Compose tokens into documents.
       *
       * @param forceDoc - If the stream contains no document, still emit a final document including any comments and directives that would be applied to a subsequent document.
       * @param endOffset - Should be set if `forceDoc` is also set, to set the document range end and to indicate errors correctly.
       */
      *compose(tokens, forceDoc = false, endOffset = -1) {
        for (const token of tokens)
          yield* this.next(token);
        yield* this.end(forceDoc, endOffset);
      }
      /** Advance the composer by one CST token. */
      *next(token) {
        if (node_process.env.LOG_STREAM)
          console.dir(token, { depth: null });
        switch (token.type) {
          case "directive":
            this.directives.add(token.source, (offset, message, warning) => {
              const pos = getErrorPos(token);
              pos[0] += offset;
              this.onError(pos, "BAD_DIRECTIVE", message, warning);
            });
            this.prelude.push(token.source);
            this.atDirectives = true;
            break;
          case "document": {
            const doc = composeDoc.composeDoc(this.options, this.directives, token, this.onError);
            if (this.atDirectives && !doc.directives.docStart)
              this.onError(token, "MISSING_CHAR", "Missing directives-end/doc-start indicator line");
            this.decorate(doc, false);
            if (this.doc)
              yield this.doc;
            this.doc = doc;
            this.atDirectives = false;
            break;
          }
          case "byte-order-mark":
          case "space":
            break;
          case "comment":
          case "newline":
            this.prelude.push(token.source);
            break;
          case "error": {
            const msg = token.source ? `${token.message}: ${JSON.stringify(token.source)}` : token.message;
            const error = new errors.YAMLParseError(getErrorPos(token), "UNEXPECTED_TOKEN", msg);
            if (this.atDirectives || !this.doc)
              this.errors.push(error);
            else
              this.doc.errors.push(error);
            break;
          }
          case "doc-end": {
            if (!this.doc) {
              const msg = "Unexpected doc-end without preceding document";
              this.errors.push(new errors.YAMLParseError(getErrorPos(token), "UNEXPECTED_TOKEN", msg));
              break;
            }
            this.doc.directives.docEnd = true;
            const end = resolveEnd.resolveEnd(token.end, token.offset + token.source.length, this.doc.options.strict, this.onError);
            this.decorate(this.doc, true);
            if (end.comment) {
              const dc = this.doc.comment;
              this.doc.comment = dc ? `${dc}
${end.comment}` : end.comment;
            }
            this.doc.range[2] = end.offset;
            break;
          }
          default:
            this.errors.push(new errors.YAMLParseError(getErrorPos(token), "UNEXPECTED_TOKEN", `Unsupported token ${token.type}`));
        }
      }
      /**
       * Call at end of input to yield any remaining document.
       *
       * @param forceDoc - If the stream contains no document, still emit a final document including any comments and directives that would be applied to a subsequent document.
       * @param endOffset - Should be set if `forceDoc` is also set, to set the document range end and to indicate errors correctly.
       */
      *end(forceDoc = false, endOffset = -1) {
        if (this.doc) {
          this.decorate(this.doc, true);
          yield this.doc;
          this.doc = null;
        } else if (forceDoc) {
          const opts = Object.assign({ _directives: this.directives }, this.options);
          const doc = new Document.Document(void 0, opts);
          if (this.atDirectives)
            this.onError(endOffset, "MISSING_CHAR", "Missing directives-end indicator line");
          doc.range = [0, endOffset, endOffset];
          this.decorate(doc, false);
          yield doc;
        }
      }
    };
    exports.Composer = Composer;
  }
});

// node_modules/yaml/dist/parse/cst-scalar.js
var require_cst_scalar = __commonJS({
  "node_modules/yaml/dist/parse/cst-scalar.js"(exports) {
    "use strict";
    var resolveBlockScalar = require_resolve_block_scalar();
    var resolveFlowScalar = require_resolve_flow_scalar();
    var errors = require_errors();
    var stringifyString = require_stringifyString();
    function resolveAsScalar(token, strict = true, onError) {
      if (token) {
        const _onError = (pos, code, message) => {
          const offset = typeof pos === "number" ? pos : Array.isArray(pos) ? pos[0] : pos.offset;
          if (onError)
            onError(offset, code, message);
          else
            throw new errors.YAMLParseError([offset, offset + 1], code, message);
        };
        switch (token.type) {
          case "scalar":
          case "single-quoted-scalar":
          case "double-quoted-scalar":
            return resolveFlowScalar.resolveFlowScalar(token, strict, _onError);
          case "block-scalar":
            return resolveBlockScalar.resolveBlockScalar({ options: { strict } }, token, _onError);
        }
      }
      return null;
    }
    function createScalarToken(value, context) {
      const { implicitKey = false, indent, inFlow = false, offset = -1, type = "PLAIN" } = context;
      const source = stringifyString.stringifyString({ type, value }, {
        implicitKey,
        indent: indent > 0 ? " ".repeat(indent) : "",
        inFlow,
        options: { blockQuote: true, lineWidth: -1 }
      });
      const end = context.end ?? [
        { type: "newline", offset: -1, indent, source: "\n" }
      ];
      switch (source[0]) {
        case "|":
        case ">": {
          const he = source.indexOf("\n");
          const head = source.substring(0, he);
          const body = source.substring(he + 1) + "\n";
          const props = [
            { type: "block-scalar-header", offset, indent, source: head }
          ];
          if (!addEndtoBlockProps(props, end))
            props.push({ type: "newline", offset: -1, indent, source: "\n" });
          return { type: "block-scalar", offset, indent, props, source: body };
        }
        case '"':
          return { type: "double-quoted-scalar", offset, indent, source, end };
        case "'":
          return { type: "single-quoted-scalar", offset, indent, source, end };
        default:
          return { type: "scalar", offset, indent, source, end };
      }
    }
    function setScalarValue(token, value, context = {}) {
      let { afterKey = false, implicitKey = false, inFlow = false, type } = context;
      let indent = "indent" in token ? token.indent : null;
      if (afterKey && typeof indent === "number")
        indent += 2;
      if (!type)
        switch (token.type) {
          case "single-quoted-scalar":
            type = "QUOTE_SINGLE";
            break;
          case "double-quoted-scalar":
            type = "QUOTE_DOUBLE";
            break;
          case "block-scalar": {
            const header = token.props[0];
            if (header.type !== "block-scalar-header")
              throw new Error("Invalid block scalar header");
            type = header.source[0] === ">" ? "BLOCK_FOLDED" : "BLOCK_LITERAL";
            break;
          }
          default:
            type = "PLAIN";
        }
      const source = stringifyString.stringifyString({ type, value }, {
        implicitKey: implicitKey || indent === null,
        indent: indent !== null && indent > 0 ? " ".repeat(indent) : "",
        inFlow,
        options: { blockQuote: true, lineWidth: -1 }
      });
      switch (source[0]) {
        case "|":
        case ">":
          setBlockScalarValue(token, source);
          break;
        case '"':
          setFlowScalarValue(token, source, "double-quoted-scalar");
          break;
        case "'":
          setFlowScalarValue(token, source, "single-quoted-scalar");
          break;
        default:
          setFlowScalarValue(token, source, "scalar");
      }
    }
    function setBlockScalarValue(token, source) {
      const he = source.indexOf("\n");
      const head = source.substring(0, he);
      const body = source.substring(he + 1) + "\n";
      if (token.type === "block-scalar") {
        const header = token.props[0];
        if (header.type !== "block-scalar-header")
          throw new Error("Invalid block scalar header");
        header.source = head;
        token.source = body;
      } else {
        const { offset } = token;
        const indent = "indent" in token ? token.indent : -1;
        const props = [
          { type: "block-scalar-header", offset, indent, source: head }
        ];
        if (!addEndtoBlockProps(props, "end" in token ? token.end : void 0))
          props.push({ type: "newline", offset: -1, indent, source: "\n" });
        for (const key of Object.keys(token))
          if (key !== "type" && key !== "offset")
            delete token[key];
        Object.assign(token, { type: "block-scalar", indent, props, source: body });
      }
    }
    function addEndtoBlockProps(props, end) {
      if (end)
        for (const st of end)
          switch (st.type) {
            case "space":
            case "comment":
              props.push(st);
              break;
            case "newline":
              props.push(st);
              return true;
          }
      return false;
    }
    function setFlowScalarValue(token, source, type) {
      switch (token.type) {
        case "scalar":
        case "double-quoted-scalar":
        case "single-quoted-scalar":
          token.type = type;
          token.source = source;
          break;
        case "block-scalar": {
          const end = token.props.slice(1);
          let oa = source.length;
          if (token.props[0].type === "block-scalar-header")
            oa -= token.props[0].source.length;
          for (const tok of end)
            tok.offset += oa;
          delete token.props;
          Object.assign(token, { type, source, end });
          break;
        }
        case "block-map":
        case "block-seq": {
          const offset = token.offset + source.length;
          const nl = { type: "newline", offset, indent: token.indent, source: "\n" };
          delete token.items;
          Object.assign(token, { type, source, end: [nl] });
          break;
        }
        default: {
          const indent = "indent" in token ? token.indent : -1;
          const end = "end" in token && Array.isArray(token.end) ? token.end.filter((st) => st.type === "space" || st.type === "comment" || st.type === "newline") : [];
          for (const key of Object.keys(token))
            if (key !== "type" && key !== "offset")
              delete token[key];
          Object.assign(token, { type, indent, source, end });
        }
      }
    }
    exports.createScalarToken = createScalarToken;
    exports.resolveAsScalar = resolveAsScalar;
    exports.setScalarValue = setScalarValue;
  }
});

// node_modules/yaml/dist/parse/cst-stringify.js
var require_cst_stringify = __commonJS({
  "node_modules/yaml/dist/parse/cst-stringify.js"(exports) {
    "use strict";
    var stringify2 = (cst) => "type" in cst ? stringifyToken(cst) : stringifyItem(cst);
    function stringifyToken(token) {
      switch (token.type) {
        case "block-scalar": {
          let res = "";
          for (const tok of token.props)
            res += stringifyToken(tok);
          return res + token.source;
        }
        case "block-map":
        case "block-seq": {
          let res = "";
          for (const item of token.items)
            res += stringifyItem(item);
          return res;
        }
        case "flow-collection": {
          let res = token.start.source;
          for (const item of token.items)
            res += stringifyItem(item);
          for (const st of token.end)
            res += st.source;
          return res;
        }
        case "document": {
          let res = stringifyItem(token);
          if (token.end)
            for (const st of token.end)
              res += st.source;
          return res;
        }
        default: {
          let res = token.source;
          if ("end" in token && token.end)
            for (const st of token.end)
              res += st.source;
          return res;
        }
      }
    }
    function stringifyItem({ start, key, sep, value }) {
      let res = "";
      for (const st of start)
        res += st.source;
      if (key)
        res += stringifyToken(key);
      if (sep)
        for (const st of sep)
          res += st.source;
      if (value)
        res += stringifyToken(value);
      return res;
    }
    exports.stringify = stringify2;
  }
});

// node_modules/yaml/dist/parse/cst-visit.js
var require_cst_visit = __commonJS({
  "node_modules/yaml/dist/parse/cst-visit.js"(exports) {
    "use strict";
    var BREAK = /* @__PURE__ */ Symbol("break visit");
    var SKIP = /* @__PURE__ */ Symbol("skip children");
    var REMOVE = /* @__PURE__ */ Symbol("remove item");
    function visit(cst, visitor) {
      if ("type" in cst && cst.type === "document")
        cst = { start: cst.start, value: cst.value };
      _visit(Object.freeze([]), cst, visitor);
    }
    visit.BREAK = BREAK;
    visit.SKIP = SKIP;
    visit.REMOVE = REMOVE;
    visit.itemAtPath = (cst, path) => {
      let item = cst;
      for (const [field, index] of path) {
        const tok = item?.[field];
        if (tok && "items" in tok) {
          item = tok.items[index];
        } else
          return void 0;
      }
      return item;
    };
    visit.parentCollection = (cst, path) => {
      const parent = visit.itemAtPath(cst, path.slice(0, -1));
      const field = path[path.length - 1][0];
      const coll = parent?.[field];
      if (coll && "items" in coll)
        return coll;
      throw new Error("Parent collection not found");
    };
    function _visit(path, item, visitor) {
      let ctrl = visitor(item, path);
      if (typeof ctrl === "symbol")
        return ctrl;
      for (const field of ["key", "value"]) {
        const token = item[field];
        if (token && "items" in token) {
          for (let i = 0; i < token.items.length; ++i) {
            const ci = _visit(Object.freeze(path.concat([[field, i]])), token.items[i], visitor);
            if (typeof ci === "number")
              i = ci - 1;
            else if (ci === BREAK)
              return BREAK;
            else if (ci === REMOVE) {
              token.items.splice(i, 1);
              i -= 1;
            }
          }
          if (typeof ctrl === "function" && field === "key")
            ctrl = ctrl(item, path);
        }
      }
      return typeof ctrl === "function" ? ctrl(item, path) : ctrl;
    }
    exports.visit = visit;
  }
});

// node_modules/yaml/dist/parse/cst.js
var require_cst = __commonJS({
  "node_modules/yaml/dist/parse/cst.js"(exports) {
    "use strict";
    var cstScalar = require_cst_scalar();
    var cstStringify = require_cst_stringify();
    var cstVisit = require_cst_visit();
    var BOM = "\uFEFF";
    var DOCUMENT = "";
    var FLOW_END = "";
    var SCALAR = "";
    var isCollection = (token) => !!token && "items" in token;
    var isScalar = (token) => !!token && (token.type === "scalar" || token.type === "single-quoted-scalar" || token.type === "double-quoted-scalar" || token.type === "block-scalar");
    function prettyToken(token) {
      switch (token) {
        case BOM:
          return "<BOM>";
        case DOCUMENT:
          return "<DOC>";
        case FLOW_END:
          return "<FLOW_END>";
        case SCALAR:
          return "<SCALAR>";
        default:
          return JSON.stringify(token);
      }
    }
    function tokenType(source) {
      switch (source) {
        case BOM:
          return "byte-order-mark";
        case DOCUMENT:
          return "doc-mode";
        case FLOW_END:
          return "flow-error-end";
        case SCALAR:
          return "scalar";
        case "---":
          return "doc-start";
        case "...":
          return "doc-end";
        case "":
        case "\n":
        case "\r\n":
          return "newline";
        case "-":
          return "seq-item-ind";
        case "?":
          return "explicit-key-ind";
        case ":":
          return "map-value-ind";
        case "{":
          return "flow-map-start";
        case "}":
          return "flow-map-end";
        case "[":
          return "flow-seq-start";
        case "]":
          return "flow-seq-end";
        case ",":
          return "comma";
      }
      switch (source[0]) {
        case " ":
        case "	":
          return "space";
        case "#":
          return "comment";
        case "%":
          return "directive-line";
        case "*":
          return "alias";
        case "&":
          return "anchor";
        case "!":
          return "tag";
        case "'":
          return "single-quoted-scalar";
        case '"':
          return "double-quoted-scalar";
        case "|":
        case ">":
          return "block-scalar-header";
      }
      return null;
    }
    exports.createScalarToken = cstScalar.createScalarToken;
    exports.resolveAsScalar = cstScalar.resolveAsScalar;
    exports.setScalarValue = cstScalar.setScalarValue;
    exports.stringify = cstStringify.stringify;
    exports.visit = cstVisit.visit;
    exports.BOM = BOM;
    exports.DOCUMENT = DOCUMENT;
    exports.FLOW_END = FLOW_END;
    exports.SCALAR = SCALAR;
    exports.isCollection = isCollection;
    exports.isScalar = isScalar;
    exports.prettyToken = prettyToken;
    exports.tokenType = tokenType;
  }
});

// node_modules/yaml/dist/parse/lexer.js
var require_lexer = __commonJS({
  "node_modules/yaml/dist/parse/lexer.js"(exports) {
    "use strict";
    var cst = require_cst();
    function isEmpty(ch) {
      switch (ch) {
        case void 0:
        case " ":
        case "\n":
        case "\r":
        case "	":
          return true;
        default:
          return false;
      }
    }
    var hexDigits = new Set("0123456789ABCDEFabcdef");
    var tagChars = new Set("0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz-#;/?:@&=+$_.!~*'()");
    var flowIndicatorChars = new Set(",[]{}");
    var invalidAnchorChars = new Set(" ,[]{}\n\r	");
    var isNotAnchorChar = (ch) => !ch || invalidAnchorChars.has(ch);
    var Lexer = class {
      constructor() {
        this.atEnd = false;
        this.blockScalarIndent = -1;
        this.blockScalarKeep = false;
        this.buffer = "";
        this.flowKey = false;
        this.flowLevel = 0;
        this.indentNext = 0;
        this.indentValue = 0;
        this.lineEndPos = null;
        this.next = null;
        this.pos = 0;
      }
      /**
       * Generate YAML tokens from the `source` string. If `incomplete`,
       * a part of the last line may be left as a buffer for the next call.
       *
       * @returns A generator of lexical tokens
       */
      *lex(source, incomplete = false) {
        if (source) {
          if (typeof source !== "string")
            throw TypeError("source is not a string");
          this.buffer = this.buffer ? this.buffer + source : source;
          this.lineEndPos = null;
        }
        this.atEnd = !incomplete;
        let next = this.next ?? "stream";
        while (next && (incomplete || this.hasChars(1)))
          next = yield* this.parseNext(next);
      }
      atLineEnd() {
        let i = this.pos;
        let ch = this.buffer[i];
        while (ch === " " || ch === "	")
          ch = this.buffer[++i];
        if (!ch || ch === "#" || ch === "\n")
          return true;
        if (ch === "\r")
          return this.buffer[i + 1] === "\n";
        return false;
      }
      charAt(n) {
        return this.buffer[this.pos + n];
      }
      continueScalar(offset) {
        let ch = this.buffer[offset];
        if (this.indentNext > 0) {
          let indent = 0;
          while (ch === " ")
            ch = this.buffer[++indent + offset];
          if (ch === "\r") {
            const next = this.buffer[indent + offset + 1];
            if (next === "\n" || !next && !this.atEnd)
              return offset + indent + 1;
          }
          return ch === "\n" || indent >= this.indentNext || !ch && !this.atEnd ? offset + indent : -1;
        }
        if (ch === "-" || ch === ".") {
          const dt = this.buffer.substr(offset, 3);
          if ((dt === "---" || dt === "...") && isEmpty(this.buffer[offset + 3]))
            return -1;
        }
        return offset;
      }
      getLine() {
        let end = this.lineEndPos;
        if (typeof end !== "number" || end !== -1 && end < this.pos) {
          end = this.buffer.indexOf("\n", this.pos);
          this.lineEndPos = end;
        }
        if (end === -1)
          return this.atEnd ? this.buffer.substring(this.pos) : null;
        if (this.buffer[end - 1] === "\r")
          end -= 1;
        return this.buffer.substring(this.pos, end);
      }
      hasChars(n) {
        return this.pos + n <= this.buffer.length;
      }
      setNext(state) {
        this.buffer = this.buffer.substring(this.pos);
        this.pos = 0;
        this.lineEndPos = null;
        this.next = state;
        return null;
      }
      peek(n) {
        return this.buffer.substr(this.pos, n);
      }
      *parseNext(next) {
        switch (next) {
          case "stream":
            return yield* this.parseStream();
          case "line-start":
            return yield* this.parseLineStart();
          case "block-start":
            return yield* this.parseBlockStart();
          case "doc":
            return yield* this.parseDocument();
          case "flow":
            return yield* this.parseFlowCollection();
          case "quoted-scalar":
            return yield* this.parseQuotedScalar();
          case "block-scalar":
            return yield* this.parseBlockScalar();
          case "plain-scalar":
            return yield* this.parsePlainScalar();
        }
      }
      *parseStream() {
        let line = this.getLine();
        if (line === null)
          return this.setNext("stream");
        if (line[0] === cst.BOM) {
          yield* this.pushCount(1);
          line = line.substring(1);
        }
        if (line[0] === "%") {
          let dirEnd = line.length;
          let cs = line.indexOf("#");
          while (cs !== -1) {
            const ch = line[cs - 1];
            if (ch === " " || ch === "	") {
              dirEnd = cs - 1;
              break;
            } else {
              cs = line.indexOf("#", cs + 1);
            }
          }
          while (true) {
            const ch = line[dirEnd - 1];
            if (ch === " " || ch === "	")
              dirEnd -= 1;
            else
              break;
          }
          const n = (yield* this.pushCount(dirEnd)) + (yield* this.pushSpaces(true));
          yield* this.pushCount(line.length - n);
          this.pushNewline();
          return "stream";
        }
        if (this.atLineEnd()) {
          const sp = yield* this.pushSpaces(true);
          yield* this.pushCount(line.length - sp);
          yield* this.pushNewline();
          return "stream";
        }
        yield cst.DOCUMENT;
        return yield* this.parseLineStart();
      }
      *parseLineStart() {
        const ch = this.charAt(0);
        if (!ch && !this.atEnd)
          return this.setNext("line-start");
        if (ch === "-" || ch === ".") {
          if (!this.atEnd && !this.hasChars(4))
            return this.setNext("line-start");
          const s = this.peek(3);
          if ((s === "---" || s === "...") && isEmpty(this.charAt(3))) {
            yield* this.pushCount(3);
            this.indentValue = 0;
            this.indentNext = 0;
            return s === "---" ? "doc" : "stream";
          }
        }
        this.indentValue = yield* this.pushSpaces(false);
        if (this.indentNext > this.indentValue && !isEmpty(this.charAt(1)))
          this.indentNext = this.indentValue;
        return yield* this.parseBlockStart();
      }
      *parseBlockStart() {
        const [ch0, ch1] = this.peek(2);
        if (!ch1 && !this.atEnd)
          return this.setNext("block-start");
        if ((ch0 === "-" || ch0 === "?" || ch0 === ":") && isEmpty(ch1)) {
          const n = (yield* this.pushCount(1)) + (yield* this.pushSpaces(true));
          this.indentNext = this.indentValue + 1;
          this.indentValue += n;
          return "block-start";
        }
        return "doc";
      }
      *parseDocument() {
        yield* this.pushSpaces(true);
        const line = this.getLine();
        if (line === null)
          return this.setNext("doc");
        let n = yield* this.pushIndicators();
        switch (line[n]) {
          case "#":
            yield* this.pushCount(line.length - n);
          // fallthrough
          case void 0:
            yield* this.pushNewline();
            return yield* this.parseLineStart();
          case "{":
          case "[":
            yield* this.pushCount(1);
            this.flowKey = false;
            this.flowLevel = 1;
            return "flow";
          case "}":
          case "]":
            yield* this.pushCount(1);
            return "doc";
          case "*":
            yield* this.pushUntil(isNotAnchorChar);
            return "doc";
          case '"':
          case "'":
            return yield* this.parseQuotedScalar();
          case "|":
          case ">":
            n += yield* this.parseBlockScalarHeader();
            n += yield* this.pushSpaces(true);
            yield* this.pushCount(line.length - n);
            yield* this.pushNewline();
            return yield* this.parseBlockScalar();
          default:
            return yield* this.parsePlainScalar();
        }
      }
      *parseFlowCollection() {
        let nl, sp;
        let indent = -1;
        do {
          nl = yield* this.pushNewline();
          if (nl > 0) {
            sp = yield* this.pushSpaces(false);
            this.indentValue = indent = sp;
          } else {
            sp = 0;
          }
          sp += yield* this.pushSpaces(true);
        } while (nl + sp > 0);
        const line = this.getLine();
        if (line === null)
          return this.setNext("flow");
        if (indent !== -1 && indent < this.indentNext && line[0] !== "#" || indent === 0 && (line.startsWith("---") || line.startsWith("...")) && isEmpty(line[3])) {
          const atFlowEndMarker = indent === this.indentNext - 1 && this.flowLevel === 1 && (line[0] === "]" || line[0] === "}");
          if (!atFlowEndMarker) {
            this.flowLevel = 0;
            yield cst.FLOW_END;
            return yield* this.parseLineStart();
          }
        }
        let n = 0;
        while (line[n] === ",") {
          n += yield* this.pushCount(1);
          n += yield* this.pushSpaces(true);
          this.flowKey = false;
        }
        n += yield* this.pushIndicators();
        switch (line[n]) {
          case void 0:
            return "flow";
          case "#":
            yield* this.pushCount(line.length - n);
            return "flow";
          case "{":
          case "[":
            yield* this.pushCount(1);
            this.flowKey = false;
            this.flowLevel += 1;
            return "flow";
          case "}":
          case "]":
            yield* this.pushCount(1);
            this.flowKey = true;
            this.flowLevel -= 1;
            return this.flowLevel ? "flow" : "doc";
          case "*":
            yield* this.pushUntil(isNotAnchorChar);
            return "flow";
          case '"':
          case "'":
            this.flowKey = true;
            return yield* this.parseQuotedScalar();
          case ":": {
            const next = this.charAt(1);
            if (this.flowKey || isEmpty(next) || next === ",") {
              this.flowKey = false;
              yield* this.pushCount(1);
              yield* this.pushSpaces(true);
              return "flow";
            }
          }
          // fallthrough
          default:
            this.flowKey = false;
            return yield* this.parsePlainScalar();
        }
      }
      *parseQuotedScalar() {
        const quote = this.charAt(0);
        let end = this.buffer.indexOf(quote, this.pos + 1);
        if (quote === "'") {
          while (end !== -1 && this.buffer[end + 1] === "'")
            end = this.buffer.indexOf("'", end + 2);
        } else {
          while (end !== -1) {
            let n = 0;
            while (this.buffer[end - 1 - n] === "\\")
              n += 1;
            if (n % 2 === 0)
              break;
            end = this.buffer.indexOf('"', end + 1);
          }
        }
        const qb = this.buffer.substring(0, end);
        let nl = qb.indexOf("\n", this.pos);
        if (nl !== -1) {
          while (nl !== -1) {
            const cs = this.continueScalar(nl + 1);
            if (cs === -1)
              break;
            nl = qb.indexOf("\n", cs);
          }
          if (nl !== -1) {
            end = nl - (qb[nl - 1] === "\r" ? 2 : 1);
          }
        }
        if (end === -1) {
          if (!this.atEnd)
            return this.setNext("quoted-scalar");
          end = this.buffer.length;
        }
        yield* this.pushToIndex(end + 1, false);
        return this.flowLevel ? "flow" : "doc";
      }
      *parseBlockScalarHeader() {
        this.blockScalarIndent = -1;
        this.blockScalarKeep = false;
        let i = this.pos;
        while (true) {
          const ch = this.buffer[++i];
          if (ch === "+")
            this.blockScalarKeep = true;
          else if (ch > "0" && ch <= "9")
            this.blockScalarIndent = Number(ch) - 1;
          else if (ch !== "-")
            break;
        }
        return yield* this.pushUntil((ch) => isEmpty(ch) || ch === "#");
      }
      *parseBlockScalar() {
        let nl = this.pos - 1;
        let indent = 0;
        let ch;
        loop: for (let i2 = this.pos; ch = this.buffer[i2]; ++i2) {
          switch (ch) {
            case " ":
              indent += 1;
              break;
            case "\n":
              nl = i2;
              indent = 0;
              break;
            case "\r": {
              const next = this.buffer[i2 + 1];
              if (!next && !this.atEnd)
                return this.setNext("block-scalar");
              if (next === "\n")
                break;
            }
            // fallthrough
            default:
              break loop;
          }
        }
        if (!ch && !this.atEnd)
          return this.setNext("block-scalar");
        if (indent >= this.indentNext) {
          if (this.blockScalarIndent === -1)
            this.indentNext = indent;
          else {
            this.indentNext = this.blockScalarIndent + (this.indentNext === 0 ? 1 : this.indentNext);
          }
          do {
            const cs = this.continueScalar(nl + 1);
            if (cs === -1)
              break;
            nl = this.buffer.indexOf("\n", cs);
          } while (nl !== -1);
          if (nl === -1) {
            if (!this.atEnd)
              return this.setNext("block-scalar");
            nl = this.buffer.length;
          }
        }
        let i = nl + 1;
        ch = this.buffer[i];
        while (ch === " ")
          ch = this.buffer[++i];
        if (ch === "	") {
          while (ch === "	" || ch === " " || ch === "\r" || ch === "\n")
            ch = this.buffer[++i];
          nl = i - 1;
        } else if (!this.blockScalarKeep) {
          do {
            let i2 = nl - 1;
            let ch2 = this.buffer[i2];
            if (ch2 === "\r")
              ch2 = this.buffer[--i2];
            const lastChar = i2;
            while (ch2 === " ")
              ch2 = this.buffer[--i2];
            if (ch2 === "\n" && i2 >= this.pos && i2 + 1 + indent > lastChar)
              nl = i2;
            else
              break;
          } while (true);
        }
        yield cst.SCALAR;
        yield* this.pushToIndex(nl + 1, true);
        return yield* this.parseLineStart();
      }
      *parsePlainScalar() {
        const inFlow = this.flowLevel > 0;
        let end = this.pos - 1;
        let i = this.pos - 1;
        let ch;
        while (ch = this.buffer[++i]) {
          if (ch === ":") {
            const next = this.buffer[i + 1];
            if (isEmpty(next) || inFlow && flowIndicatorChars.has(next))
              break;
            end = i;
          } else if (isEmpty(ch)) {
            let next = this.buffer[i + 1];
            if (ch === "\r") {
              if (next === "\n") {
                i += 1;
                ch = "\n";
                next = this.buffer[i + 1];
              } else
                end = i;
            }
            if (next === "#" || inFlow && flowIndicatorChars.has(next))
              break;
            if (ch === "\n") {
              const cs = this.continueScalar(i + 1);
              if (cs === -1)
                break;
              i = Math.max(i, cs - 2);
            }
          } else {
            if (inFlow && flowIndicatorChars.has(ch))
              break;
            end = i;
          }
        }
        if (!ch && !this.atEnd)
          return this.setNext("plain-scalar");
        yield cst.SCALAR;
        yield* this.pushToIndex(end + 1, true);
        return inFlow ? "flow" : "doc";
      }
      *pushCount(n) {
        if (n > 0) {
          yield this.buffer.substr(this.pos, n);
          this.pos += n;
          return n;
        }
        return 0;
      }
      *pushToIndex(i, allowEmpty) {
        const s = this.buffer.slice(this.pos, i);
        if (s) {
          yield s;
          this.pos += s.length;
          return s.length;
        } else if (allowEmpty)
          yield "";
        return 0;
      }
      *pushIndicators() {
        let n = 0;
        loop: while (true) {
          switch (this.charAt(0)) {
            case "!":
              n += yield* this.pushTag();
              n += yield* this.pushSpaces(true);
              continue loop;
            case "&":
              n += yield* this.pushUntil(isNotAnchorChar);
              n += yield* this.pushSpaces(true);
              continue loop;
            case "-":
            // this is an error
            case "?":
            // this is an error outside flow collections
            case ":": {
              const inFlow = this.flowLevel > 0;
              const ch1 = this.charAt(1);
              if (isEmpty(ch1) || inFlow && flowIndicatorChars.has(ch1)) {
                if (!inFlow)
                  this.indentNext = this.indentValue + 1;
                else if (this.flowKey)
                  this.flowKey = false;
                n += yield* this.pushCount(1);
                n += yield* this.pushSpaces(true);
                continue loop;
              }
            }
          }
          break loop;
        }
        return n;
      }
      *pushTag() {
        if (this.charAt(1) === "<") {
          let i = this.pos + 2;
          let ch = this.buffer[i];
          while (!isEmpty(ch) && ch !== ">")
            ch = this.buffer[++i];
          return yield* this.pushToIndex(ch === ">" ? i + 1 : i, false);
        } else {
          let i = this.pos + 1;
          let ch = this.buffer[i];
          while (ch) {
            if (tagChars.has(ch))
              ch = this.buffer[++i];
            else if (ch === "%" && hexDigits.has(this.buffer[i + 1]) && hexDigits.has(this.buffer[i + 2])) {
              ch = this.buffer[i += 3];
            } else
              break;
          }
          return yield* this.pushToIndex(i, false);
        }
      }
      *pushNewline() {
        const ch = this.buffer[this.pos];
        if (ch === "\n")
          return yield* this.pushCount(1);
        else if (ch === "\r" && this.charAt(1) === "\n")
          return yield* this.pushCount(2);
        else
          return 0;
      }
      *pushSpaces(allowTabs) {
        let i = this.pos - 1;
        let ch;
        do {
          ch = this.buffer[++i];
        } while (ch === " " || allowTabs && ch === "	");
        const n = i - this.pos;
        if (n > 0) {
          yield this.buffer.substr(this.pos, n);
          this.pos = i;
        }
        return n;
      }
      *pushUntil(test) {
        let i = this.pos;
        let ch = this.buffer[i];
        while (!test(ch))
          ch = this.buffer[++i];
        return yield* this.pushToIndex(i, false);
      }
    };
    exports.Lexer = Lexer;
  }
});

// node_modules/yaml/dist/parse/line-counter.js
var require_line_counter = __commonJS({
  "node_modules/yaml/dist/parse/line-counter.js"(exports) {
    "use strict";
    var LineCounter = class {
      constructor() {
        this.lineStarts = [];
        this.addNewLine = (offset) => this.lineStarts.push(offset);
        this.linePos = (offset) => {
          let low = 0;
          let high = this.lineStarts.length;
          while (low < high) {
            const mid = low + high >> 1;
            if (this.lineStarts[mid] < offset)
              low = mid + 1;
            else
              high = mid;
          }
          if (this.lineStarts[low] === offset)
            return { line: low + 1, col: 1 };
          if (low === 0)
            return { line: 0, col: offset };
          const start = this.lineStarts[low - 1];
          return { line: low, col: offset - start + 1 };
        };
      }
    };
    exports.LineCounter = LineCounter;
  }
});

// node_modules/yaml/dist/parse/parser.js
var require_parser = __commonJS({
  "node_modules/yaml/dist/parse/parser.js"(exports) {
    "use strict";
    var node_process = __require("process");
    var cst = require_cst();
    var lexer = require_lexer();
    function includesToken(list, type) {
      for (let i = 0; i < list.length; ++i)
        if (list[i].type === type)
          return true;
      return false;
    }
    function findNonEmptyIndex(list) {
      for (let i = 0; i < list.length; ++i) {
        switch (list[i].type) {
          case "space":
          case "comment":
          case "newline":
            break;
          default:
            return i;
        }
      }
      return -1;
    }
    function isFlowToken(token) {
      switch (token?.type) {
        case "alias":
        case "scalar":
        case "single-quoted-scalar":
        case "double-quoted-scalar":
        case "flow-collection":
          return true;
        default:
          return false;
      }
    }
    function getPrevProps(parent) {
      switch (parent.type) {
        case "document":
          return parent.start;
        case "block-map": {
          const it = parent.items[parent.items.length - 1];
          return it.sep ?? it.start;
        }
        case "block-seq":
          return parent.items[parent.items.length - 1].start;
        /* istanbul ignore next should not happen */
        default:
          return [];
      }
    }
    function getFirstKeyStartProps(prev) {
      if (prev.length === 0)
        return [];
      let i = prev.length;
      loop: while (--i >= 0) {
        switch (prev[i].type) {
          case "doc-start":
          case "explicit-key-ind":
          case "map-value-ind":
          case "seq-item-ind":
          case "newline":
            break loop;
        }
      }
      while (prev[++i]?.type === "space") {
      }
      return prev.splice(i, prev.length);
    }
    function arrayPushArray(target, source) {
      if (source.length < 1e5)
        Array.prototype.push.apply(target, source);
      else
        for (let i = 0; i < source.length; ++i)
          target.push(source[i]);
    }
    function fixFlowSeqItems(fc) {
      if (fc.start.type === "flow-seq-start") {
        for (const it of fc.items) {
          if (it.sep && !it.value && !includesToken(it.start, "explicit-key-ind") && !includesToken(it.sep, "map-value-ind")) {
            if (it.key)
              it.value = it.key;
            delete it.key;
            if (isFlowToken(it.value)) {
              if (it.value.end)
                arrayPushArray(it.value.end, it.sep);
              else
                it.value.end = it.sep;
            } else
              arrayPushArray(it.start, it.sep);
            delete it.sep;
          }
        }
      }
    }
    var Parser = class {
      /**
       * @param onNewLine - If defined, called separately with the start position of
       *   each new line (in `parse()`, including the start of input).
       */
      constructor(onNewLine) {
        this.atNewLine = true;
        this.atScalar = false;
        this.indent = 0;
        this.offset = 0;
        this.onKeyLine = false;
        this.stack = [];
        this.source = "";
        this.type = "";
        this.lexer = new lexer.Lexer();
        this.onNewLine = onNewLine;
      }
      /**
       * Parse `source` as a YAML stream.
       * If `incomplete`, a part of the last line may be left as a buffer for the next call.
       *
       * Errors are not thrown, but yielded as `{ type: 'error', message }` tokens.
       *
       * @returns A generator of tokens representing each directive, document, and other structure.
       */
      *parse(source, incomplete = false) {
        if (this.onNewLine && this.offset === 0)
          this.onNewLine(0);
        for (const lexeme of this.lexer.lex(source, incomplete))
          yield* this.next(lexeme);
        if (!incomplete)
          yield* this.end();
      }
      /**
       * Advance the parser by the `source` of one lexical token.
       */
      *next(source) {
        this.source = source;
        if (node_process.env.LOG_TOKENS)
          console.log("|", cst.prettyToken(source));
        if (this.atScalar) {
          this.atScalar = false;
          yield* this.step();
          this.offset += source.length;
          return;
        }
        const type = cst.tokenType(source);
        if (!type) {
          const message = `Not a YAML token: ${source}`;
          yield* this.pop({ type: "error", offset: this.offset, message, source });
          this.offset += source.length;
        } else if (type === "scalar") {
          this.atNewLine = false;
          this.atScalar = true;
          this.type = "scalar";
        } else {
          this.type = type;
          yield* this.step();
          switch (type) {
            case "newline":
              this.atNewLine = true;
              this.indent = 0;
              if (this.onNewLine)
                this.onNewLine(this.offset + source.length);
              break;
            case "space":
              if (this.atNewLine && source[0] === " ")
                this.indent += source.length;
              break;
            case "explicit-key-ind":
            case "map-value-ind":
            case "seq-item-ind":
              if (this.atNewLine)
                this.indent += source.length;
              break;
            case "doc-mode":
            case "flow-error-end":
              return;
            default:
              this.atNewLine = false;
          }
          this.offset += source.length;
        }
      }
      /** Call at end of input to push out any remaining constructions */
      *end() {
        while (this.stack.length > 0)
          yield* this.pop();
      }
      get sourceToken() {
        const st = {
          type: this.type,
          offset: this.offset,
          indent: this.indent,
          source: this.source
        };
        return st;
      }
      *step() {
        const top = this.peek(1);
        if (this.type === "doc-end" && top?.type !== "doc-end") {
          while (this.stack.length > 0)
            yield* this.pop();
          this.stack.push({
            type: "doc-end",
            offset: this.offset,
            source: this.source
          });
          return;
        }
        if (!top)
          return yield* this.stream();
        switch (top.type) {
          case "document":
            return yield* this.document(top);
          case "alias":
          case "scalar":
          case "single-quoted-scalar":
          case "double-quoted-scalar":
            return yield* this.scalar(top);
          case "block-scalar":
            return yield* this.blockScalar(top);
          case "block-map":
            return yield* this.blockMap(top);
          case "block-seq":
            return yield* this.blockSequence(top);
          case "flow-collection":
            return yield* this.flowCollection(top);
          case "doc-end":
            return yield* this.documentEnd(top);
        }
        yield* this.pop();
      }
      peek(n) {
        return this.stack[this.stack.length - n];
      }
      *pop(error) {
        const token = error ?? this.stack.pop();
        if (!token) {
          const message = "Tried to pop an empty stack";
          yield { type: "error", offset: this.offset, source: "", message };
        } else if (this.stack.length === 0) {
          yield token;
        } else {
          const top = this.peek(1);
          if (token.type === "block-scalar") {
            token.indent = "indent" in top ? top.indent : 0;
          } else if (token.type === "flow-collection" && top.type === "document") {
            token.indent = 0;
          }
          if (token.type === "flow-collection")
            fixFlowSeqItems(token);
          switch (top.type) {
            case "document":
              top.value = token;
              break;
            case "block-scalar":
              top.props.push(token);
              break;
            case "block-map": {
              const it = top.items[top.items.length - 1];
              if (it.value) {
                top.items.push({ start: [], key: token, sep: [] });
                this.onKeyLine = true;
                return;
              } else if (it.sep) {
                it.value = token;
              } else {
                Object.assign(it, { key: token, sep: [] });
                this.onKeyLine = !it.explicitKey;
                return;
              }
              break;
            }
            case "block-seq": {
              const it = top.items[top.items.length - 1];
              if (it.value)
                top.items.push({ start: [], value: token });
              else
                it.value = token;
              break;
            }
            case "flow-collection": {
              const it = top.items[top.items.length - 1];
              if (!it || it.value)
                top.items.push({ start: [], key: token, sep: [] });
              else if (it.sep)
                it.value = token;
              else
                Object.assign(it, { key: token, sep: [] });
              return;
            }
            /* istanbul ignore next should not happen */
            default:
              yield* this.pop();
              yield* this.pop(token);
          }
          if ((top.type === "document" || top.type === "block-map" || top.type === "block-seq") && (token.type === "block-map" || token.type === "block-seq")) {
            const last = token.items[token.items.length - 1];
            if (last && !last.sep && !last.value && last.start.length > 0 && findNonEmptyIndex(last.start) === -1 && (token.indent === 0 || last.start.every((st) => st.type !== "comment" || st.indent < token.indent))) {
              if (top.type === "document")
                top.end = last.start;
              else
                top.items.push({ start: last.start });
              token.items.splice(-1, 1);
            }
          }
        }
      }
      *stream() {
        switch (this.type) {
          case "directive-line":
            yield { type: "directive", offset: this.offset, source: this.source };
            return;
          case "byte-order-mark":
          case "space":
          case "comment":
          case "newline":
            yield this.sourceToken;
            return;
          case "doc-mode":
          case "doc-start": {
            const doc = {
              type: "document",
              offset: this.offset,
              start: []
            };
            if (this.type === "doc-start")
              doc.start.push(this.sourceToken);
            this.stack.push(doc);
            return;
          }
        }
        yield {
          type: "error",
          offset: this.offset,
          message: `Unexpected ${this.type} token in YAML stream`,
          source: this.source
        };
      }
      *document(doc) {
        if (doc.value)
          return yield* this.lineEnd(doc);
        switch (this.type) {
          case "doc-start": {
            if (findNonEmptyIndex(doc.start) !== -1) {
              yield* this.pop();
              yield* this.step();
            } else
              doc.start.push(this.sourceToken);
            return;
          }
          case "anchor":
          case "tag":
          case "space":
          case "comment":
          case "newline":
            doc.start.push(this.sourceToken);
            return;
        }
        const bv = this.startBlockValue(doc);
        if (bv)
          this.stack.push(bv);
        else {
          yield {
            type: "error",
            offset: this.offset,
            message: `Unexpected ${this.type} token in YAML document`,
            source: this.source
          };
        }
      }
      *scalar(scalar) {
        if (this.type === "map-value-ind") {
          const prev = getPrevProps(this.peek(2));
          const start = getFirstKeyStartProps(prev);
          let sep;
          if (scalar.end) {
            sep = scalar.end;
            sep.push(this.sourceToken);
            delete scalar.end;
          } else
            sep = [this.sourceToken];
          const map = {
            type: "block-map",
            offset: scalar.offset,
            indent: scalar.indent,
            items: [{ start, key: scalar, sep }]
          };
          this.onKeyLine = true;
          this.stack[this.stack.length - 1] = map;
        } else
          yield* this.lineEnd(scalar);
      }
      *blockScalar(scalar) {
        switch (this.type) {
          case "space":
          case "comment":
          case "newline":
            scalar.props.push(this.sourceToken);
            return;
          case "scalar":
            scalar.source = this.source;
            this.atNewLine = true;
            this.indent = 0;
            if (this.onNewLine) {
              let nl = this.source.indexOf("\n") + 1;
              while (nl !== 0) {
                this.onNewLine(this.offset + nl);
                nl = this.source.indexOf("\n", nl) + 1;
              }
            }
            yield* this.pop();
            break;
          /* istanbul ignore next should not happen */
          default:
            yield* this.pop();
            yield* this.step();
        }
      }
      *blockMap(map) {
        const it = map.items[map.items.length - 1];
        switch (this.type) {
          case "newline":
            this.onKeyLine = false;
            if (it.value) {
              const end = "end" in it.value ? it.value.end : void 0;
              const last = Array.isArray(end) ? end[end.length - 1] : void 0;
              if (last?.type === "comment")
                end?.push(this.sourceToken);
              else
                map.items.push({ start: [this.sourceToken] });
            } else if (it.sep) {
              it.sep.push(this.sourceToken);
            } else {
              it.start.push(this.sourceToken);
            }
            return;
          case "space":
          case "comment":
            if (it.value) {
              map.items.push({ start: [this.sourceToken] });
            } else if (it.sep) {
              it.sep.push(this.sourceToken);
            } else {
              if (this.atIndentedComment(it.start, map.indent)) {
                const prev = map.items[map.items.length - 2];
                const end = prev?.value?.end;
                if (Array.isArray(end)) {
                  arrayPushArray(end, it.start);
                  end.push(this.sourceToken);
                  map.items.pop();
                  return;
                }
              }
              it.start.push(this.sourceToken);
            }
            return;
        }
        if (this.indent >= map.indent) {
          const atMapIndent = !this.onKeyLine && this.indent === map.indent;
          const atNextItem = atMapIndent && (it.sep || it.explicitKey) && this.type !== "seq-item-ind";
          let start = [];
          if (atNextItem && it.sep && !it.value) {
            const nl = [];
            for (let i = 0; i < it.sep.length; ++i) {
              const st = it.sep[i];
              switch (st.type) {
                case "newline":
                  nl.push(i);
                  break;
                case "space":
                  break;
                case "comment":
                  if (st.indent > map.indent)
                    nl.length = 0;
                  break;
                default:
                  nl.length = 0;
              }
            }
            if (nl.length >= 2)
              start = it.sep.splice(nl[1]);
          }
          switch (this.type) {
            case "anchor":
            case "tag":
              if (atNextItem || it.value) {
                start.push(this.sourceToken);
                map.items.push({ start });
                this.onKeyLine = true;
              } else if (it.sep) {
                it.sep.push(this.sourceToken);
              } else {
                it.start.push(this.sourceToken);
              }
              return;
            case "explicit-key-ind":
              if (!it.sep && !it.explicitKey) {
                it.start.push(this.sourceToken);
                it.explicitKey = true;
              } else if (atNextItem || it.value) {
                start.push(this.sourceToken);
                map.items.push({ start, explicitKey: true });
              } else {
                this.stack.push({
                  type: "block-map",
                  offset: this.offset,
                  indent: this.indent,
                  items: [{ start: [this.sourceToken], explicitKey: true }]
                });
              }
              this.onKeyLine = true;
              return;
            case "map-value-ind":
              if (it.explicitKey) {
                if (!it.sep) {
                  if (includesToken(it.start, "newline")) {
                    Object.assign(it, { key: null, sep: [this.sourceToken] });
                  } else {
                    const start2 = getFirstKeyStartProps(it.start);
                    this.stack.push({
                      type: "block-map",
                      offset: this.offset,
                      indent: this.indent,
                      items: [{ start: start2, key: null, sep: [this.sourceToken] }]
                    });
                  }
                } else if (it.value) {
                  map.items.push({ start: [], key: null, sep: [this.sourceToken] });
                } else if (includesToken(it.sep, "map-value-ind")) {
                  this.stack.push({
                    type: "block-map",
                    offset: this.offset,
                    indent: this.indent,
                    items: [{ start, key: null, sep: [this.sourceToken] }]
                  });
                } else if (isFlowToken(it.key) && !includesToken(it.sep, "newline")) {
                  const start2 = getFirstKeyStartProps(it.start);
                  const key = it.key;
                  const sep = it.sep;
                  sep.push(this.sourceToken);
                  delete it.key;
                  delete it.sep;
                  this.stack.push({
                    type: "block-map",
                    offset: this.offset,
                    indent: this.indent,
                    items: [{ start: start2, key, sep }]
                  });
                } else if (start.length > 0) {
                  it.sep = it.sep.concat(start, this.sourceToken);
                } else {
                  it.sep.push(this.sourceToken);
                }
              } else {
                if (!it.sep) {
                  Object.assign(it, { key: null, sep: [this.sourceToken] });
                } else if (it.value || atNextItem) {
                  map.items.push({ start, key: null, sep: [this.sourceToken] });
                } else if (includesToken(it.sep, "map-value-ind")) {
                  this.stack.push({
                    type: "block-map",
                    offset: this.offset,
                    indent: this.indent,
                    items: [{ start: [], key: null, sep: [this.sourceToken] }]
                  });
                } else {
                  it.sep.push(this.sourceToken);
                }
              }
              this.onKeyLine = true;
              return;
            case "alias":
            case "scalar":
            case "single-quoted-scalar":
            case "double-quoted-scalar": {
              const fs = this.flowScalar(this.type);
              if (atNextItem || it.value) {
                map.items.push({ start, key: fs, sep: [] });
                this.onKeyLine = true;
              } else if (it.sep) {
                this.stack.push(fs);
              } else {
                Object.assign(it, { key: fs, sep: [] });
                this.onKeyLine = true;
              }
              return;
            }
            default: {
              const bv = this.startBlockValue(map);
              if (bv) {
                if (bv.type === "block-seq") {
                  if (!it.explicitKey && it.sep && !includesToken(it.sep, "newline")) {
                    yield* this.pop({
                      type: "error",
                      offset: this.offset,
                      message: "Unexpected block-seq-ind on same line with key",
                      source: this.source
                    });
                    return;
                  }
                } else if (atMapIndent) {
                  map.items.push({ start });
                }
                this.stack.push(bv);
                return;
              }
            }
          }
        }
        yield* this.pop();
        yield* this.step();
      }
      *blockSequence(seq) {
        const it = seq.items[seq.items.length - 1];
        switch (this.type) {
          case "newline":
            if (it.value) {
              const end = "end" in it.value ? it.value.end : void 0;
              const last = Array.isArray(end) ? end[end.length - 1] : void 0;
              if (last?.type === "comment")
                end?.push(this.sourceToken);
              else
                seq.items.push({ start: [this.sourceToken] });
            } else
              it.start.push(this.sourceToken);
            return;
          case "space":
          case "comment":
            if (it.value)
              seq.items.push({ start: [this.sourceToken] });
            else {
              if (this.atIndentedComment(it.start, seq.indent)) {
                const prev = seq.items[seq.items.length - 2];
                const end = prev?.value?.end;
                if (Array.isArray(end)) {
                  arrayPushArray(end, it.start);
                  end.push(this.sourceToken);
                  seq.items.pop();
                  return;
                }
              }
              it.start.push(this.sourceToken);
            }
            return;
          case "anchor":
          case "tag":
            if (it.value || this.indent <= seq.indent)
              break;
            it.start.push(this.sourceToken);
            return;
          case "seq-item-ind":
            if (this.indent !== seq.indent)
              break;
            if (it.value || includesToken(it.start, "seq-item-ind"))
              seq.items.push({ start: [this.sourceToken] });
            else
              it.start.push(this.sourceToken);
            return;
        }
        if (this.indent > seq.indent) {
          const bv = this.startBlockValue(seq);
          if (bv) {
            this.stack.push(bv);
            return;
          }
        }
        yield* this.pop();
        yield* this.step();
      }
      *flowCollection(fc) {
        const it = fc.items[fc.items.length - 1];
        if (this.type === "flow-error-end") {
          let top;
          do {
            yield* this.pop();
            top = this.peek(1);
          } while (top?.type === "flow-collection");
        } else if (fc.end.length === 0) {
          switch (this.type) {
            case "comma":
            case "explicit-key-ind":
              if (!it || it.sep)
                fc.items.push({ start: [this.sourceToken] });
              else
                it.start.push(this.sourceToken);
              return;
            case "map-value-ind":
              if (!it || it.value)
                fc.items.push({ start: [], key: null, sep: [this.sourceToken] });
              else if (it.sep)
                it.sep.push(this.sourceToken);
              else
                Object.assign(it, { key: null, sep: [this.sourceToken] });
              return;
            case "space":
            case "comment":
            case "newline":
            case "anchor":
            case "tag":
              if (!it || it.value)
                fc.items.push({ start: [this.sourceToken] });
              else if (it.sep)
                it.sep.push(this.sourceToken);
              else
                it.start.push(this.sourceToken);
              return;
            case "alias":
            case "scalar":
            case "single-quoted-scalar":
            case "double-quoted-scalar": {
              const fs = this.flowScalar(this.type);
              if (!it || it.value)
                fc.items.push({ start: [], key: fs, sep: [] });
              else if (it.sep)
                this.stack.push(fs);
              else
                Object.assign(it, { key: fs, sep: [] });
              return;
            }
            case "flow-map-end":
            case "flow-seq-end":
              fc.end.push(this.sourceToken);
              return;
          }
          const bv = this.startBlockValue(fc);
          if (bv)
            this.stack.push(bv);
          else {
            yield* this.pop();
            yield* this.step();
          }
        } else {
          const parent = this.peek(2);
          if (parent.type === "block-map" && (this.type === "map-value-ind" && parent.indent === fc.indent || this.type === "newline" && !parent.items[parent.items.length - 1].sep)) {
            yield* this.pop();
            yield* this.step();
          } else if (this.type === "map-value-ind" && parent.type !== "flow-collection") {
            const prev = getPrevProps(parent);
            const start = getFirstKeyStartProps(prev);
            fixFlowSeqItems(fc);
            const sep = fc.end.splice(1, fc.end.length);
            sep.push(this.sourceToken);
            const map = {
              type: "block-map",
              offset: fc.offset,
              indent: fc.indent,
              items: [{ start, key: fc, sep }]
            };
            this.onKeyLine = true;
            this.stack[this.stack.length - 1] = map;
          } else {
            yield* this.lineEnd(fc);
          }
        }
      }
      flowScalar(type) {
        if (this.onNewLine) {
          let nl = this.source.indexOf("\n") + 1;
          while (nl !== 0) {
            this.onNewLine(this.offset + nl);
            nl = this.source.indexOf("\n", nl) + 1;
          }
        }
        return {
          type,
          offset: this.offset,
          indent: this.indent,
          source: this.source
        };
      }
      startBlockValue(parent) {
        switch (this.type) {
          case "alias":
          case "scalar":
          case "single-quoted-scalar":
          case "double-quoted-scalar":
            return this.flowScalar(this.type);
          case "block-scalar-header":
            return {
              type: "block-scalar",
              offset: this.offset,
              indent: this.indent,
              props: [this.sourceToken],
              source: ""
            };
          case "flow-map-start":
          case "flow-seq-start":
            return {
              type: "flow-collection",
              offset: this.offset,
              indent: this.indent,
              start: this.sourceToken,
              items: [],
              end: []
            };
          case "seq-item-ind":
            return {
              type: "block-seq",
              offset: this.offset,
              indent: this.indent,
              items: [{ start: [this.sourceToken] }]
            };
          case "explicit-key-ind": {
            this.onKeyLine = true;
            const prev = getPrevProps(parent);
            const start = getFirstKeyStartProps(prev);
            start.push(this.sourceToken);
            return {
              type: "block-map",
              offset: this.offset,
              indent: this.indent,
              items: [{ start, explicitKey: true }]
            };
          }
          case "map-value-ind": {
            this.onKeyLine = true;
            const prev = getPrevProps(parent);
            const start = getFirstKeyStartProps(prev);
            return {
              type: "block-map",
              offset: this.offset,
              indent: this.indent,
              items: [{ start, key: null, sep: [this.sourceToken] }]
            };
          }
        }
        return null;
      }
      atIndentedComment(start, indent) {
        if (this.type !== "comment")
          return false;
        if (this.indent <= indent)
          return false;
        return start.every((st) => st.type === "newline" || st.type === "space");
      }
      *documentEnd(docEnd) {
        if (this.type !== "doc-mode") {
          if (docEnd.end)
            docEnd.end.push(this.sourceToken);
          else
            docEnd.end = [this.sourceToken];
          if (this.type === "newline")
            yield* this.pop();
        }
      }
      *lineEnd(token) {
        switch (this.type) {
          case "comma":
          case "doc-start":
          case "doc-end":
          case "flow-seq-end":
          case "flow-map-end":
          case "map-value-ind":
            yield* this.pop();
            yield* this.step();
            break;
          case "newline":
            this.onKeyLine = false;
          // fallthrough
          case "space":
          case "comment":
          default:
            if (token.end)
              token.end.push(this.sourceToken);
            else
              token.end = [this.sourceToken];
            if (this.type === "newline")
              yield* this.pop();
        }
      }
    };
    exports.Parser = Parser;
  }
});

// node_modules/yaml/dist/public-api.js
var require_public_api = __commonJS({
  "node_modules/yaml/dist/public-api.js"(exports) {
    "use strict";
    var composer = require_composer();
    var Document = require_Document();
    var errors = require_errors();
    var log = require_log();
    var identity = require_identity();
    var lineCounter = require_line_counter();
    var parser = require_parser();
    function parseOptions(options) {
      const prettyErrors = options.prettyErrors !== false;
      const lineCounter$1 = options.lineCounter || prettyErrors && new lineCounter.LineCounter() || null;
      return { lineCounter: lineCounter$1, prettyErrors };
    }
    function parseAllDocuments(source, options = {}) {
      const { lineCounter: lineCounter2, prettyErrors } = parseOptions(options);
      const parser$1 = new parser.Parser(lineCounter2?.addNewLine);
      const composer$1 = new composer.Composer(options);
      const docs = Array.from(composer$1.compose(parser$1.parse(source)));
      if (prettyErrors && lineCounter2)
        for (const doc of docs) {
          doc.errors.forEach(errors.prettifyError(source, lineCounter2));
          doc.warnings.forEach(errors.prettifyError(source, lineCounter2));
        }
      if (docs.length > 0)
        return docs;
      return Object.assign([], { empty: true }, composer$1.streamInfo());
    }
    function parseDocument3(source, options = {}) {
      const { lineCounter: lineCounter2, prettyErrors } = parseOptions(options);
      const parser$1 = new parser.Parser(lineCounter2?.addNewLine);
      const composer$1 = new composer.Composer(options);
      let doc = null;
      for (const _doc of composer$1.compose(parser$1.parse(source), true, source.length)) {
        if (!doc)
          doc = _doc;
        else if (doc.options.logLevel !== "silent") {
          doc.errors.push(new errors.YAMLParseError(_doc.range.slice(0, 2), "MULTIPLE_DOCS", "Source contains multiple documents; please use YAML.parseAllDocuments()"));
          break;
        }
      }
      if (prettyErrors && lineCounter2) {
        doc.errors.forEach(errors.prettifyError(source, lineCounter2));
        doc.warnings.forEach(errors.prettifyError(source, lineCounter2));
      }
      return doc;
    }
    function parse(src, reviver, options) {
      let _reviver = void 0;
      if (typeof reviver === "function") {
        _reviver = reviver;
      } else if (options === void 0 && reviver && typeof reviver === "object") {
        options = reviver;
      }
      const doc = parseDocument3(src, options);
      if (!doc)
        return null;
      doc.warnings.forEach((warning) => log.warn(doc.options.logLevel, warning));
      if (doc.errors.length > 0) {
        if (doc.options.logLevel !== "silent")
          throw doc.errors[0];
        else
          doc.errors = [];
      }
      return doc.toJS(Object.assign({ reviver: _reviver }, options));
    }
    function stringify2(value, replacer, options) {
      let _replacer = null;
      if (typeof replacer === "function" || Array.isArray(replacer)) {
        _replacer = replacer;
      } else if (options === void 0 && replacer) {
        options = replacer;
      }
      if (typeof options === "string")
        options = options.length;
      if (typeof options === "number") {
        const indent = Math.round(options);
        options = indent < 1 ? void 0 : indent > 8 ? { indent: 8 } : { indent };
      }
      if (value === void 0) {
        const { keepUndefined } = options ?? replacer ?? {};
        if (!keepUndefined)
          return void 0;
      }
      if (identity.isDocument(value) && !_replacer)
        return value.toString(options);
      return new Document.Document(value, _replacer, options).toString(options);
    }
    exports.parse = parse;
    exports.parseAllDocuments = parseAllDocuments;
    exports.parseDocument = parseDocument3;
    exports.stringify = stringify2;
  }
});

// node_modules/yaml/dist/index.js
var require_dist = __commonJS({
  "node_modules/yaml/dist/index.js"(exports) {
    "use strict";
    var composer = require_composer();
    var Document = require_Document();
    var Schema = require_Schema();
    var errors = require_errors();
    var Alias = require_Alias();
    var identity = require_identity();
    var Pair = require_Pair();
    var Scalar = require_Scalar();
    var YAMLMap = require_YAMLMap();
    var YAMLSeq = require_YAMLSeq();
    var cst = require_cst();
    var lexer = require_lexer();
    var lineCounter = require_line_counter();
    var parser = require_parser();
    var publicApi = require_public_api();
    var visit = require_visit();
    exports.Composer = composer.Composer;
    exports.Document = Document.Document;
    exports.Schema = Schema.Schema;
    exports.YAMLError = errors.YAMLError;
    exports.YAMLParseError = errors.YAMLParseError;
    exports.YAMLWarning = errors.YAMLWarning;
    exports.Alias = Alias.Alias;
    exports.isAlias = identity.isAlias;
    exports.isCollection = identity.isCollection;
    exports.isDocument = identity.isDocument;
    exports.isMap = identity.isMap;
    exports.isNode = identity.isNode;
    exports.isPair = identity.isPair;
    exports.isScalar = identity.isScalar;
    exports.isSeq = identity.isSeq;
    exports.Pair = Pair.Pair;
    exports.Scalar = Scalar.Scalar;
    exports.YAMLMap = YAMLMap.YAMLMap;
    exports.YAMLSeq = YAMLSeq.YAMLSeq;
    exports.CST = cst;
    exports.Lexer = lexer.Lexer;
    exports.LineCounter = lineCounter.LineCounter;
    exports.Parser = parser.Parser;
    exports.parse = publicApi.parse;
    exports.parseAllDocuments = publicApi.parseAllDocuments;
    exports.parseDocument = publicApi.parseDocument;
    exports.stringify = publicApi.stringify;
    exports.visit = visit.visit;
    exports.visitAsync = visit.visitAsync;
  }
});

// companion/ideas.ts
var import_yaml = __toESM(require_dist());
import { readFileSync, writeFileSync, appendFileSync, renameSync, mkdirSync, existsSync, readdirSync, unlinkSync } from "node:fs";
import { join, resolve, dirname, relative } from "node:path";
import { execFileSync, spawn, spawnSync } from "node:child_process";
import { createServer } from "node:http";
import { createHash, randomBytes } from "node:crypto";
import { argv, exit, cwd, pid, platform, env } from "node:process";

// companion/manifests.ts
var ENGINE_RELATIVE = ".companion/companion.mjs";
var ENGINE_DIR = ENGINE_RELATIVE.slice(0, ENGINE_RELATIVE.lastIndexOf("/"));
var anyCase = (names) => [...new Set(names.flatMap((n) => [n, n.toLowerCase()]))];
var SHELL_TOOLS = anyCase(["Bash", "PowerShell", "pwsh", "shell", "local_shell"]);
var READ_TOOLS = anyCase(["Read", "read_file", "readfile"]);
var CURSOR_WRITE_TOOLS = anyCase(["Write", "StrReplace", "Delete", "EditNotebook", "ApplyPatch", "search_replace"]);

// companion/ideas.ts
var ENGINE_CMD = `node ${ENGINE_RELATIVE}`;
var STATUSES = ["todo", "doing", "done", "blocked"];
var IDEAS_DIR = (projectDir) => join(resolve(projectDir), "ideas");
function paths(projectDir) {
  const ideas = IDEAS_DIR(projectDir);
  return {
    graph: join(ideas, "graph.yaml"),
    html: join(ideas, "graph.html"),
    log: join(ideas, "log.md"),
    worklist: join(ideas, ".scan-todo"),
    done: join(ideas, ".scan-done"),
    approved: join(ideas, ".approved"),
    runtime: join(ideas, ".runtime")
  };
}
var graphPath = (projectDir) => paths(projectDir).graph;
function load(file) {
  if (!existsSync(file)) {
    throw new Error(`no idea graph at ${file} \u2014 run \`${ENGINE_CMD} init\` first`);
  }
  const doc = (0, import_yaml.parseDocument)(readFileSync(file, "utf8"));
  if (doc.errors.length > 0) throw new Error(`invalid YAML in ${file}: ${doc.errors[0].message}`);
  const graph = doc.toJSON();
  if (!graph || !Array.isArray(graph.ideas)) throw new Error(`${file} has no \`ideas:\` list`);
  return { doc, graph };
}
function pauseSync(ms) {
  Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, ms);
}
function atomicWrite(file, text) {
  const tmp = `${file}.${pid}.${Math.random().toString(36).slice(2, 8)}.tmp`;
  writeFileSync(tmp, text);
  for (let attempt = 0; ; attempt++) {
    try {
      renameSync(tmp, file);
      return;
    } catch (error) {
      const code = error.code ?? "";
      if (attempt >= 5 || !["EPERM", "EBUSY", "EACCES"].includes(code)) {
        rmFileQuietly(tmp);
        throw error;
      }
      pauseSync(20 * (attempt + 1));
    }
  }
}
function save(file, doc) {
  atomicWrite(file, String(doc));
}
var byId = (g) => new Map(g.ideas.map((i) => [i.id, i]));
function dependents(g, id) {
  return g.ideas.filter((i) => (i.needs ?? []).includes(id)).map((i) => i.id).sort();
}
function frontier(g) {
  const map = byId(g);
  return g.ideas.filter((i) => (i.status ?? "todo") === "todo" && (i.needs ?? []).every((n) => map.get(n)?.status === "done"));
}
function findCycle(g) {
  const map = byId(g);
  const state = /* @__PURE__ */ new Map();
  const path = [];
  let cycle = [];
  const walk2 = (id) => {
    if (state.get(id) === 1) {
      cycle = [...path.slice(path.indexOf(id)), id];
      return true;
    }
    if (state.get(id) === 2) return false;
    state.set(id, 1);
    path.push(id);
    for (const need of map.get(id)?.needs ?? []) {
      if (map.has(need) && walk2(need)) return true;
    }
    path.pop();
    state.set(id, 2);
    return false;
  };
  for (const i of g.ideas) if (walk2(i.id)) break;
  return cycle;
}
function orphans(g) {
  const ends = new Set(g.endpoints ?? []);
  if (ends.size === 0) return [];
  const map = byId(g);
  const reaching = /* @__PURE__ */ new Set();
  const stack = [...ends];
  while (stack.length > 0) {
    const id = stack.pop();
    if (reaching.has(id) || !map.has(id)) continue;
    reaching.add(id);
    stack.push(...map.get(id).needs ?? []);
  }
  return g.ideas.filter((i) => !reaching.has(i.id)).map((i) => i.id).sort();
}
var worklistFile = (projectDir) => paths(projectDir).worklist;
var logFile = (projectDir) => paths(projectDir).log;
var SKIP_RULES = [
  [/\.(png|jpe?g|gif|svg|ico|webp|pdf|zip|gz|tar|woff2?|ttf|eot|mp[34]|mov|wasm)$/i, "\u4E8C\u8FDB\u5236\u6216\u8D44\u6E90\u6587\u4EF6\uFF0C\u8BFB\u5B83\u8BFB\u4E0D\u51FA\u5185\u5BB9"],
  [/\.(lock|min\.js|map)$|(^|\/)package-lock\.json$/i, "\u751F\u6210\u7269\uFF08\u9501\u6587\u4EF6 / \u538B\u7F29\u4EA7\u7269 / source map\uFF09\uFF0C\u6E90\u5934\u5728\u522B\u5904"],
  [/(^|\/)(node_modules|vendor|third_party)\//i, "\u7B2C\u4E09\u65B9\u4F9D\u8D56\uFF0C\u4E0D\u662F\u8FD9\u4E2A\u9879\u76EE\u81EA\u5DF1\u7684\u4EE3\u7801"],
  [/(^|\/)ideas\//i, "\u8D26\u672C\u76EE\u5F55\uFF0C\u7531\u5F15\u64CE\u81EA\u5DF1\u751F\u6210\u548C\u7EF4\u62A4"]
];
function scanIgnores(projectDir) {
  const file = join(projectDir, "ideas", ".scanignore");
  if (!existsSync(file)) return [];
  return readFileSync(file, "utf8").split("\n").map((l) => l.trim()).filter((l) => l && !l.startsWith("#"));
}
function allProjectFiles(projectDir) {
  try {
    return execFileSync(
      "git",
      ["ls-files", "-z", "--cached", "--others", "--exclude-standard"],
      { cwd: projectDir, encoding: "utf8" }
    ).split("\0").filter(Boolean);
  } catch {
    return walk(projectDir, projectDir);
  }
}
function skipReason(file, ignores) {
  const prefix = ignores.find((p) => file.startsWith(p));
  if (prefix) return `ideas/.scanignore \u91CC\u6392\u9664\u7684\u524D\u7F00 ${prefix}`;
  return SKIP_RULES.find(([rule]) => rule.test(file))?.[1] ?? null;
}
function skippedFiles(projectDir) {
  const ignores = scanIgnores(projectDir);
  return allProjectFiles(projectDir).flatMap((file) => {
    const reason = skipReason(file, ignores);
    return reason ? [{ file, reason }] : [];
  }).sort((a, b) => a.file < b.file ? -1 : a.file > b.file ? 1 : 0);
}
function listProjectFiles(projectDir) {
  const ignores = scanIgnores(projectDir);
  return allProjectFiles(projectDir).filter((f) => skipReason(f, ignores) === null).sort();
}
var IGNORE_DIRS = /* @__PURE__ */ new Set([".git", "node_modules", "dist", "build", ".venv", "__pycache__", ".next", "target"]);
function walk(dir, root) {
  const out = [];
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    if (entry.name.startsWith(".") && entry.name !== ".claude") continue;
    if (IGNORE_DIRS.has(entry.name)) continue;
    const full = join(dir, entry.name);
    if (entry.isDirectory()) out.push(...walk(full, root));
    else out.push(relative(root, full).replaceAll("\\", "/"));
  }
  return out;
}
var doneFile = (projectDir) => paths(projectDir).done;
function readChecklist(projectDir) {
  const file = worklistFile(projectDir);
  if (!existsSync(file)) return [];
  return readFileSync(file, "utf8").split("\n").map((l) => l.trim()).filter(Boolean);
}
function readStruck(projectDir) {
  const file = doneFile(projectDir);
  const map = /* @__PURE__ */ new Map();
  if (!existsSync(file)) return map;
  for (const line of readFileSync(file, "utf8").split("\n")) {
    const t = line.trim();
    if (!t) continue;
    const tab = t.indexOf("	");
    if (tab < 0) map.set(t.toLowerCase(), null);
    else map.set(t.slice(0, tab).toLowerCase(), t.slice(tab + 1));
  }
  return map;
}
function contentHash(projectDir, rel) {
  try {
    return sha256(readFileSync(join(resolve(projectDir), rel), "utf8"));
  } catch {
    return "unreadable";
  }
}
function effectivelyStruck(projectDir, struck, file) {
  const hash = struck.get(file.toLowerCase());
  if (hash === void 0) return false;
  if (hash === null || hash === "unreadable") return true;
  return hash === contentHash(projectDir, file);
}
function readWorklist(projectDir) {
  const struck = readStruck(projectDir);
  if (struck.size === 0) return readChecklist(projectDir);
  return readChecklist(projectDir).filter((f) => !effectivelyStruck(projectDir, struck, f));
}
function worklistDone(projectDir) {
  const struck = readStruck(projectDir);
  if (struck.size === 0) return 0;
  return readChecklist(projectDir).filter((f) => effectivelyStruck(projectDir, struck, f)).length;
}
function reconcileWorklist(projectDir, all) {
  const checklist = readChecklist(projectDir);
  const known = new Set(checklist.map((f) => f.toLowerCase()));
  const live = new Set(all.map((f) => f.toLowerCase()));
  const added = all.filter((f) => !known.has(f.toLowerCase()));
  const removed = checklist.filter((f) => !live.has(f.toLowerCase()));
  if (added.length === 0 && removed.length === 0) return { added, removed };
  mkdirSync(dirname(worklistFile(projectDir)), { recursive: true });
  atomicWrite(worklistFile(projectDir), all.join("\n") + (all.length > 0 ? "\n" : ""));
  return { added, removed };
}
function writeWorklist(projectDir, files) {
  mkdirSync(dirname(worklistFile(projectDir)), { recursive: true });
  atomicWrite(worklistFile(projectDir), files.join("\n") + (files.length > 0 ? "\n" : ""));
  atomicWrite(doneFile(projectDir), "");
}
function strike(projectDir, filePath) {
  const checklist = readChecklist(projectDir);
  if (checklist.length === 0) return -1;
  const target = relative(projectDir, resolve(filePath)).replaceAll("\\", "/");
  const key = target.toLowerCase();
  if (!checklist.some((f) => f.toLowerCase() === key)) return -1;
  const struck = readStruck(projectDir);
  if (effectivelyStruck(projectDir, struck, target)) return -1;
  appendFileSync(doneFile(projectDir), `${target}	${contentHash(projectDir, target)}
`);
  const after = readStruck(projectDir);
  return checklist.filter((f) => !effectivelyStruck(projectDir, after, f)).length;
}
var PLANNING_FIELDS = ["what", "why", "expected", "how", "why_this_way", "future"];
function badPlanPath(path) {
  const posix = String(path).replaceAll("\\", "/");
  if (/^\//.test(posix) || /^[A-Za-z]:/.test(posix)) return "\u5FC5\u987B\u5199\u6210\u9879\u76EE\u76F8\u5BF9\u8DEF\u5F84\uFF0C\u4E0D\u80FD\u662F\u7EDD\u5BF9\u8DEF\u5F84";
  if (posix.split("/").includes("..")) return "\u4E0D\u80FD\u542B `..`\uFF08\u7236\u76EE\u5F55\uFF09\u6BB5 \u2014\u2014 \u90A3\u6307\u5411\u9879\u76EE\u4E4B\u5916";
  return null;
}
function lineCount(text) {
  if (text.length === 0) return 0;
  return text.replace(/\r?\n$/, "").split("\n").length;
}
function badLineRange(fullPath, lines) {
  const impossible = (why) => ({ kind: "impossible", why });
  const shape = `\u884C\u53F7\u8981\u5199\u6210 start-end\uFF08\u5982 12-40\uFF09\uFF0C\u5355\u884C\u5199\u884C\u53F7\uFF0C\u591A\u6BB5\u7528\u9017\u53F7\u9694\u5F00\uFF08\u5982 12-40,88\uFF09\uFF0C\u73B0\u5728\u662F\u300C${lines}\u300D`;
  const segments = String(lines).trim().split(",").map((s) => s.trim());
  if (segments.length === 0) return impossible(shape);
  let count;
  try {
    count = lineCount(readFileSync(fullPath, "utf8"));
  } catch {
    count = null;
  }
  let stale = null;
  for (const segment of segments) {
    const m = /^(\d+)(?:-(\d+))?$/.exec(segment);
    if (!m) return impossible(shape);
    const start = Number(m[1]);
    const end = m[2] === void 0 ? start : Number(m[2]);
    if (start < 1) return impossible(`\u884C\u53F7\u4ECE 1 \u8D77\u7B97\uFF0Cstart \u4E0D\u80FD\u662F ${start}`);
    if (end < start) return impossible(`end ${end} \u5C0F\u4E8E start ${start}`);
    if (count !== null && end > count && !stale) {
      stale = { kind: "stale", why: `end ${end} \u8D85\u8FC7\u6587\u4EF6\u73B0\u5728\u53EA\u6709\u7684 ${count} \u884C` };
    }
  }
  return stale;
}
function check(g, projectDir, file) {
  const stamp = legacyStamp(g);
  if (stamp && (file === void 0 || sameFile(file, graphPath(projectDir)))) {
    return { errors: [legacyAtCanonical(stamp)], warnings: [] };
  }
  const errors = [];
  const warnings = [];
  const seen = /* @__PURE__ */ new Set();
  const map = byId(g);
  for (const idea of g.ideas) {
    const at = idea.id || "(missing id)";
    if (!idea.id) errors.push(`an idea has no id`);
    else if (seen.has(idea.id)) errors.push(`${at}: duplicate id`);
    seen.add(idea.id);
    if (!idea.name) errors.push(`${at}: no name \u2014 the graph shows names, so it needs one`);
    const status = idea.status ?? "todo";
    if (!STATUSES.includes(status)) errors.push(`${at}: unknown status "${status}"`);
    for (const need of idea.needs ?? []) {
      if (!map.has(need)) errors.push(`${at}: needs unknown idea "${need}"`);
      if (need === idea.id) errors.push(`${at}: needs itself`);
    }
    for (const field of PLANNING_FIELDS) {
      if (!idea[field]) warnings.push(`${at}: unanswered \u2014 ${field}`);
    }
    if (status === "done") {
      if (!idea.code?.length) errors.push(`${at}: done but no \`code\` \u2014 where is it?`);
      if (!idea.verify) errors.push(`${at}: done but no \`verify\` \u2014 how was it confirmed?`);
      if (idea.verify?.manual && !idea.verify.signed_off) {
        errors.push(`${at}: done on a manual check with no \`signed_off\` \u2014 a human must sign it`);
      }
      if (!idea.code?.some((c) => c.lines)) {
        warnings.push(`${at}: done but no line numbers in \`code\``);
      }
    }
    if (status === "doing" && idea.verify?.command && !(idea.verify.test_files ?? []).length) {
      warnings.push(`${at}: \u5728\u505A\uFF0C\u4F46 \`verify\` \u53EA\u6709\u4E00\u6761\u547D\u4EE4\u3001\u6CA1\u6709 \`test_files\` \u2014\u2014 \u6CA1\u6709\u80FD\u5355\u72EC\u5931\u8D25\u7684\u6D4B\u8BD5\u5C31\u6491\u4E0D\u8D77 RED\uFF0C\u5B9E\u73B0\u524D\u8981\u4E48\u8865\u4E0A\u6D4B\u8BD5\u6587\u4EF6\uFF0C\u8981\u4E48\u8BF7\u4EBA\u6279\u4E00\u6B21 red-waiver\uFF08D8\uFF09`);
    }
    for (const ref of idea.code ?? []) {
      if (!ref.file) {
        errors.push(`${at}: a \`code\` entry has no file`);
        continue;
      }
      const strayed = badPlanPath(ref.file);
      if (strayed) {
        errors.push(`${at}: \`code\` \u8DEF\u5F84 ${ref.file} ${strayed}\uFF08D31\uFF09`);
        continue;
      }
      const full = resolve(projectDir, ref.file);
      if (status === "done" && !existsSync(full)) {
        errors.push(`${at}: code file not found \u2014 ${ref.file}`);
        continue;
      }
      if (status === "done" && ref.lines !== void 0) {
        const bad = badLineRange(full, ref.lines);
        if (bad?.kind === "impossible") {
          errors.push(`${at}: \`code\` ${ref.file} \u7684\u884C\u53F7\u5BF9\u4E0D\u4E0A\uFF1A${bad.why}\uFF08D32\uFF09`);
        } else if (bad) {
          warnings.push(`${at}: \`code\` ${ref.file} \u7684\u884C\u53F7\u8FC7\u671F\u4E86\uFF1A${bad.why} \u2014\u2014 \u522B\u5904\u7684\u6539\u52A8\u628A\u5B83\u6539\u77ED\u4E86\uFF0C\u8BB0\u5F55\u8BE5\u5237\u65B0\uFF1A\u5728 ideas/graph.yaml \u91CC\u628A\u8FD9\u6761 \`lines\` \u6539\u6210\u73B0\u5728\u7684\u8303\u56F4\uFF0C\u518D\u8DD1 \`${ENGINE_CMD} check\` \u590D\u6838\uFF08D32\uFF09`);
        }
      }
    }
    for (const rel of idea.verify?.test_files ?? []) {
      const strayed = badPlanPath(rel);
      if (strayed) errors.push(`${at}: \`verify.test_files\` \u8DEF\u5F84 ${rel} ${strayed}\uFF08D31\uFF09`);
    }
  }
  const cycle = findCycle(g);
  if (cycle.length > 0) {
    errors.push(`cycle: ${cycle.join(" \u2192 ")} \u2014 these are one idea, merge them`);
  }
  for (const end of g.endpoints ?? []) {
    if (!map.has(end)) errors.push(`endpoint "${end}" is not an idea`);
  }
  if (!g.endpoints?.length) warnings.push(`no \`endpoints\` \u2014 nothing defines "done" for this project`);
  for (const id of orphans(g)) warnings.push(`${id}: no endpoint depends on this, directly or not`);
  const unread = readWorklist(projectDir);
  if (unread.length > 0) {
    warnings.push(`\u626B\u63CF\u672A\u5B8C\u6210\uFF1A\u8FD8\u6709 ${unread.length} \u4E2A\u6587\u4EF6\u6CA1\u88AB\u8BFB\u8FC7\uFF08\`${ENGINE_CMD} scan\`\uFF09`);
  }
  return { errors, warnings };
}
var PLAN_FIELDS = ["what", "why", "expected", "how", "why_this_way", "future"];
function isBuildReady(idea) {
  for (const field of PLAN_FIELDS) {
    if (!String(idea[field] ?? "").trim()) return `missing ${field}`;
  }
  if (!(idea.code ?? []).some((c) => c.file)) return "missing code.file (where the implementation will live)";
  if (!idea.verify?.command && !idea.verify?.manual) return "missing verify";
  return null;
}
function needsUnmet(idea, graph) {
  const map = byId(graph);
  const unmet = (idea.needs ?? []).filter((n) => (map.get(n)?.status ?? "todo") !== "done");
  return unmet.length ? `waiting on ${unmet.join(", ")}` : null;
}
var claimedFiles = (idea) => [
  ...(idea.code ?? []).map((c) => c.file).filter(Boolean),
  ...idea.verify?.test_files ?? []
];
var sameFile = (a, b) => {
  const norm2 = (p) => p.replaceAll("\\", "/").replace(/\/+$/, "");
  return platform === "win32" ? norm2(a).toLowerCase() === norm2(b).toLowerCase() : norm2(a) === norm2(b);
};
function fileClash(idea, graph) {
  const mine = claimedFiles(idea);
  for (const other of graph.ideas) {
    if (other.id === idea.id || other.status !== "doing") continue;
    const shared = claimedFiles(other).filter((f) => mine.some((m) => sameFile(m, f)));
    if (shared.length) return `overlapping doing files \u2014 ${other.id}: ${shared.join(", ")}`;
  }
  return null;
}
function writeNextId(doc, value) {
  const top = doc.contents;
  const had = (top.items ?? []).some((p) => p.key?.value === "next_id");
  doc.setIn(["next_id"], value);
  if (!had && top.items) {
    const added = top.items.pop();
    const at = top.items.findIndex((p) => p.key?.value === "ideas");
    top.items.splice(at < 0 ? top.items.length : at, 0, added);
  }
}
function addIdea(doc, graph, name, needs, date) {
  for (const n2 of needs) {
    if (!graph.ideas.some((i) => i.id === n2)) throw new Error(`\u672A\u77E5\u524D\u7F6E ${n2}`);
  }
  const highest = graph.ideas.reduce((m, i) => Math.max(m, idNumber(i.id) || 0), 0);
  const n = Number(graph.next_id) || highest + 1;
  const id = formatId(n);
  doc.setIn(["ideas", graph.ideas.length], {
    id,
    name,
    status: "todo",
    ...needs.length ? { needs } : {},
    log: [{ date, by: "new", note: "\u521B\u5EFA" }]
  });
  writeNextId(doc, n + 1);
  return id;
}
var sha256 = (text) => createHash("sha256").update(text.replaceAll("\r\n", "\n")).digest("hex");
function approvalSnapshot(graph, gate, nodeIds) {
  const map = byId(graph);
  const three = (i) => ({
    id: i.id,
    name: i.name,
    needs: i.needs ?? [],
    what: i.what ?? "",
    why: i.why ?? "",
    expected: i.expected ?? ""
  });
  const projection = gate === "decomposition" ? graph.ideas.map(three) : (nodeIds ?? []).map((id) => {
    const i = map.get(id);
    if (!i) throw new Error(`no idea with id ${id}`);
    return {
      ...three(i),
      how: i.how ?? "",
      why_this_way: i.why_this_way ?? "",
      future: i.future ?? "",
      code: i.code ?? [],
      verify: i.verify ?? {}
    };
  });
  return sha256(JSON.stringify(projection)).slice(0, 12);
}
var pendingDir = (projectDir) => join(paths(projectDir).runtime, "pending");
var approvalsDir = (projectDir) => join(paths(projectDir).runtime, "approvals");
function requestApproval(projectDir, graph, gate, nodeIds, meta = {}) {
  if (gate !== "decomposition" && !nodeIds?.length) {
    throw new Error(`${gate} \u5173\u5361\u5FC5\u987B\u70B9\u540D\u60F3\u6CD5\uFF08nodeIds\uFF09`);
  }
  if (gate === "manual-check") {
    for (const id of nodeIds) {
      const idea = byId(graph).get(id);
      if (!idea?.verify?.manual) throw new Error(`${id} \u7684\u9A8C\u8BC1\u4E0D\u662F\u4EBA\u5DE5\u68C0\u67E5\uFF08manual\uFF09\u2014\u2014manual-check \u5173\u5361\u53EA\u7B7E\u4EBA\u5DE5\u9A8C\u6536`);
      if (idea.verify.signed_off) throw new Error(`${id} \u5DF2\u7ECF\u6709\u4EBA\u7B7E\u8FC7\u5B57\u4E86\uFF0C\u4E0D\u80FD\u8986\u76D6`);
    }
  }
  const snapshot = approvalSnapshot(graph, gate, nodeIds);
  const challenge = `CC-${randomBytes(4).toString("hex").toUpperCase()}`;
  const file = join(pendingDir(projectDir), `${challenge}.json`);
  mkdirSync(pendingDir(projectDir), { recursive: true });
  atomicWrite(file, JSON.stringify({
    v: 1,
    challenge,
    gate,
    node_ids: nodeIds ?? null,
    snapshot,
    requested_at: meta.date ?? "",
    by: meta.by ?? ""
  }, null, 2));
  return { challenge, gate, file };
}
var ANSWER = /^\s*(批准|同意|APPROVE|拒绝|REJECT)\s+(CC-[A-Fa-f0-9]{8})\s*[。.!！]?\s*$/;
function applyApproval(projectDir, prompt, meta) {
  const m = ANSWER.exec(prompt ?? "");
  if (!m) return null;
  const decision = /^(批准|同意|APPROVE)$/i.test(m[1]) ? "approved" : "rejected";
  const challenge = m[2].toUpperCase();
  const file = join(pendingDir(projectDir), `${challenge}.json`);
  if (!existsSync(file)) return { ok: false, reason: `\u53E3\u4EE4 ${challenge} \u4E0D\u5B58\u5728\u6216\u5DF2\u7528\u8FC7 \u2014\u2014 \u91CD\u65B0 request-approval` };
  const pending = JSON.parse(readFileSync(file, "utf8"));
  const { graph } = load(graphPath(projectDir));
  let current;
  try {
    current = approvalSnapshot(graph, pending.gate, pending.node_ids ?? void 0);
  } catch {
    current = "<node-gone>";
  }
  if (current !== pending.snapshot) {
    rmFileQuietly(file);
    return { ok: false, reason: "\u88AB\u6279\u7684\u5185\u5BB9\u5728\u8BF7\u6C42\u4E4B\u540E\u88AB\u6539\u8FC7\u4E86\uFF0C\u53E3\u4EE4\u4F5C\u5E9F \u2014\u2014 \u91CD\u65B0 request-approval" };
  }
  mkdirSync(approvalsDir(projectDir), { recursive: true });
  atomicWrite(join(approvalsDir(projectDir), `${challenge}.json`), JSON.stringify({
    ...pending,
    decision,
    responded_at: meta.date,
    session_id: meta.session_id ?? "",
    turn_id: meta.turn_id ?? "",
    prompt_sha256: sha256(prompt)
  }, null, 2));
  rmFileQuietly(file);
  if (decision === "approved" && pending.gate === "manual-check") {
    const { doc, graph: g } = load(graphPath(projectDir));
    for (const id of pending.node_ids ?? []) {
      const index = g.ideas.findIndex((i) => i.id === id);
      if (index < 0) continue;
      doc.setIn(
        ["ideas", index, "verify", "signed_off"],
        `${pending.by || "\u4EBA"} ${meta.date} \u2014\u2014 \u7ECF\u4E00\u6B21\u6027\u53E3\u4EE4 ${challenge} \u6279\u51C6\uFF1B\u56DE\u6267 ideas/.runtime/approvals/${challenge}.json`
      );
    }
    save(graphPath(projectDir), doc);
  }
  return { ok: true, decision, gate: pending.gate };
}
function rmFileQuietly(file) {
  try {
    unlinkSync(file);
  } catch {
  }
}
function validApproval(projectDir, graph, gate, nodeIds) {
  const dirPath = approvalsDir(projectDir);
  if (!existsSync(dirPath)) return false;
  let want;
  try {
    want = approvalSnapshot(graph, gate, nodeIds);
  } catch {
    return false;
  }
  const sameIds = (a, b) => JSON.stringify([...a ?? []].sort()) === JSON.stringify([...b ?? []].sort());
  for (const name of readdirSync(dirPath)) {
    try {
      const r = JSON.parse(readFileSync(join(dirPath, name), "utf8"));
      if (r.decision === "approved" && r.gate === gate && r.snapshot === want && sameIds(r.node_ids, nodeIds)) return true;
    } catch {
    }
  }
  return false;
}
var evidenceFile = (projectDir, id) => join(paths(projectDir).runtime, `${id}.json`);
function readEvidence(projectDir, id) {
  const file = evidenceFile(projectDir, id);
  if (!existsSync(file)) return {};
  try {
    return JSON.parse(readFileSync(file, "utf8"));
  } catch {
    return {};
  }
}
function writeEvidence(projectDir, id, evidence) {
  mkdirSync(paths(projectDir).runtime, { recursive: true });
  atomicWrite(evidenceFile(projectDir, id), JSON.stringify(evidence, null, 2));
}
function hashTests(projectDir, idea) {
  const out = {};
  for (const rel of idea.verify?.test_files ?? []) {
    const full = join(resolve(projectDir), rel);
    out[rel] = existsSync(full) ? sha256(readFileSync(full, "utf8")) : "missing";
  }
  return out;
}
function presentTests(projectDir, idea) {
  return (idea.verify?.test_files ?? []).filter((rel) => existsSync(join(resolve(projectDir), rel)));
}
var sameHashes = (a, b) => JSON.stringify(Object.entries(a).sort()) === JSON.stringify(Object.entries(b).sort());
var NOT_FOUND_EXITS = platform === "win32" ? [9009, 127] : [127];
var SHELL_OPERATORS = /[|&;<>`$(){}\n]/;
function missingExecutable(projectDir, command) {
  if (SHELL_OPERATORS.test(command)) return void 0;
  const token = (command.trim().match(/^"([^"]+)"|^'([^']+)'|^(\S+)/) ?? []).slice(1).find(Boolean);
  if (!token) return void 0;
  if (/[\\/]/.test(token)) {
    return existsSync(resolve(projectDir, token)) ? void 0 : `\u627E\u4E0D\u5230\u53EF\u6267\u884C\u6587\u4EF6 ${token}`;
  }
  const exts = platform === "win32" ? ["", ...(env.PATHEXT ?? ".COM;.EXE;.BAT;.CMD").split(";")] : [""];
  for (const dir of (env.PATH ?? "").split(platform === "win32" ? ";" : ":")) {
    if (!dir) continue;
    for (const ext of exts) if (ext !== void 0 && existsSync(join(dir, token + ext))) return void 0;
  }
  return `\u547D\u4EE4\u6CA1\u627E\u5230\uFF1A${token} \u4E0D\u5728 PATH \u4E0A`;
}
function infraReason(run) {
  const error = run.error;
  if (error) {
    return error.code === "ETIMEDOUT" ? "\u547D\u4EE4\u8D85\u65F6\uFF0C\u88AB\u5F3A\u884C\u6740\u6389" : `\u547D\u4EE4\u6CA1\u80FD\u542F\u52A8\uFF1A${error.message}`;
  }
  if (run.signal) return `\u547D\u4EE4\u88AB\u4FE1\u53F7 ${run.signal} \u6740\u6389`;
  if (run.status === null) return "\u547D\u4EE4\u6CA1\u6709\u7559\u4E0B\u9000\u51FA\u7801\uFF08\u8D85\u65F6\u6216\u88AB\u6740\uFF09";
  if (NOT_FOUND_EXITS.includes(run.status)) return `\u547D\u4EE4\u6CA1\u627E\u5230\uFF08\u9000\u51FA\u7801 ${run.status}\uFF09`;
  return void 0;
}
function infraOf(run) {
  if (run.outcome === "infra_error") return run.infra_error ?? "\u547D\u4EE4\u6CA1\u80FD\u771F\u6B63\u8DD1\u8D77\u6765";
  if (run.exit_code === -1) return "\u547D\u4EE4\u6CA1\u6709\u7559\u4E0B\u9000\u51FA\u7801\uFF08\u8D85\u65F6\u6216\u88AB\u6740\uFF09";
  if (NOT_FOUND_EXITS.includes(run.exit_code)) return `\u547D\u4EE4\u6CA1\u627E\u5230\uFF08\u9000\u51FA\u7801 ${run.exit_code}\uFF09`;
  return void 0;
}
function recordChange(projectDir, graph, filePath) {
  const root = resolve(projectDir).replaceAll("\\", "/");
  const rel = resolve(projectDir, filePath).replaceAll("\\", "/").replace(new RegExp(`^${root.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}/`, platform === "win32" ? "i" : ""), "");
  for (const idea of graph.ideas) {
    if (!(idea.code ?? []).some((c) => c.file && sameFile(c.file, rel))) continue;
    const evidence = readEvidence(projectDir, idea.id);
    evidence.change_seq = (evidence.change_seq ?? 0) + 1;
    writeEvidence(projectDir, idea.id, evidence);
  }
}
function redGateReady(projectDir, graph, id) {
  const idea = byId(graph).get(id);
  if (!idea) return { ready: false, reason: `no idea with id ${id}` };
  if (!idea.verify?.command) return { ready: true, reason: "manual-only" };
  const evidence = readEvidence(projectDir, id);
  if (!evidence.red) {
    return { ready: false, reason: `\u8FD8\u6CA1\u6709 RED \u8BB0\u5F55 \u2014\u2014 \u5148 run-check ${id} --phase red\uFF0C\u770B\u7740\u6D4B\u8BD5\u771F\u7684\u5931\u8D25` };
  }
  const broken = infraOf(evidence.red);
  if (broken) {
    return { ready: false, reason: `\u4E0A\u6B21 red \u91CC\u9A8C\u8BC1\u547D\u4EE4\u6839\u672C\u6CA1\u8DD1\u8D77\u6765\uFF1A${broken} \u2014\u2014 \u8FD9\u662F\u547D\u4EE4\u574F\u4E86\uFF0C\u4E0D\u662F\u6D4B\u8BD5\u7EA2\u4E86\uFF1B\u4FEE\u597D verify.command \u518D\u91CD\u8DD1 run-check ${id} --phase red` };
  }
  if (!sameHashes(evidence.red.test_hashes, hashTests(projectDir, idea))) {
    return { ready: false, reason: `RED \u8BC1\u636E\u5DF2\u8FC7\u671F\uFF08stale\uFF09\u2014\u2014 \u6D4B\u8BD5\u6587\u4EF6\u5728\u5931\u8D25\u8BB0\u5F55\u4E4B\u540E\u88AB\u6539\u8FC7\uFF0C\u91CD\u8DD1 red` };
  }
  const declared = idea.verify.test_files ?? [];
  if (presentTests(projectDir, idea).length === 0) {
    if (validApproval(projectDir, graph, "red-waiver", [id])) {
      return { ready: true, reason: "\u6CA1\u6709\u53EF\u5931\u8D25\u7684\u6D4B\u8BD5\u6587\u4EF6 + \u4EBA\u6279\u7684\u8C41\u514D" };
    }
    const gap = declared.length === 0 ? `${id} \u6CA1\u6709 verify.test_files` : `${id} \u58F0\u660E\u7684\u6D4B\u8BD5\u6587\u4EF6\u4E00\u4E2A\u90FD\u4E0D\u5B58\u5728\uFF08${declared.join("\u3001")}\uFF09`;
    return {
      ready: false,
      reason: `${gap} \u2014\u2014 \u6CA1\u6709\u6D4B\u8BD5\u6587\u4EF6\u65F6\uFF0C\u9A8C\u8BC1\u547D\u4EE4\u7684\u975E\u96F6\u9000\u51FA\u53EF\u80FD\u6765\u81EA\u4EFB\u4F55\u5730\u65B9\uFF0C\u6491\u4E0D\u8D77\u4E00\u6B21 RED\uFF08D8\uFF09\u3002\u4E24\u6761\u51FA\u8DEF\uFF1A\u628A\u4F1A\u5931\u8D25\u7684\u6D4B\u8BD5\u5199\u51FA\u6765\u3001\u5199\u8FDB verify.test_files \u518D run-check ${id} --phase red\uFF0C\u6216\u8005\u8BF7\u4EBA\u6279\u4E00\u6B21\u8C41\u514D\uFF08request-approval --gate red-waiver --node ${id}\uFF09`
    };
  }
  if (evidence.red.outcome === "unexpected_pass") {
    return validApproval(projectDir, graph, "red-waiver", [id]) ? { ready: true, reason: "unexpected_pass + \u4EBA\u6279\u7684\u8C41\u514D" } : { ready: false, reason: `\u6D4B\u8BD5\u610F\u5916\u5148\u7EFF\uFF08unexpected_pass\uFF09\u2014\u2014 \u9700\u8981\u4EBA\u6279\u4E00\u6B21 red-waiver\uFF08request-approval --gate red-waiver --node ${id}\uFF09` };
  }
  return { ready: true };
}
function greenCurrent(projectDir, graph, id) {
  const idea = byId(graph).get(id);
  if (!idea?.verify?.command) return false;
  const evidence = readEvidence(projectDir, id);
  if (!evidence.green || evidence.green.exit_code !== 0) return false;
  if (evidence.green.at_seq !== (evidence.change_seq ?? 0)) return false;
  return sameHashes(evidence.green.test_hashes, hashTests(projectDir, idea));
}
function decideProductWrite(projectDir, graph, filePath) {
  const root = resolve(projectDir).replaceAll("\\", "/");
  const full = resolve(projectDir, filePath).replaceAll("\\", "/");
  const inRoot = platform === "win32" ? full.toLowerCase().startsWith(root.toLowerCase() + "/") : full.startsWith(root + "/");
  const rel = inRoot ? full.slice(root.length + 1) : full;
  const p = paths(projectDir);
  for (const [file, label] of [
    [p.approved, "\u6279\u51C6\u8BB0\u5F55"],
    [p.worklist, "\u626B\u63CF\u6E05\u5355"],
    [p.done, "\u5DF2\u8BFB\u8BB0\u5F55"],
    [p.html, "\u751F\u6210\u7684\u7F51\u9875"]
  ]) {
    if (sameFile(full, file.replaceAll("\\", "/"))) {
      return { allow: false, reason: `${label}\u53EA\u80FD\u7531 CLI \u548C hook \u4EA7\u751F\uFF0C\u8C01\u90FD\u4E0D\u8BB8\u76F4\u63A5\u5199\uFF08D24\uFF09\u3002` };
    }
  }
  const relLower = platform === "win32" ? rel.toLowerCase() : rel;
  if (relLower === "ideas/.runtime" || relLower.startsWith("ideas/.runtime/")) {
    return { allow: false, reason: "ideas/.runtime/ \u91CC\u662F\u7A0B\u5E8F\u4FDD\u7BA1\u7684\u8BC1\u636E\uFF08\u6279\u51C6\u56DE\u6267\u3001\u6D4B\u8BD5\u7EA2\u7EFF\u8BB0\u5F55\uFF09\uFF0C\u53EA\u80FD\u7531 CLI \u4EA7\u751F\uFF08D24\uFF09\u3002\u8981\u7559\u8BC1\u636E\uFF1Arun-check / request-approval\u3002" };
  }
  if (/^ideas\/graph\.[^/]+\.ya?ml$/i.test(rel)) {
    return { allow: false, reason: `${rel} \u662F\u8FC1\u79FB\u8F93\u5165\uFF0C\u8FC1\u79FB\u540E\u53EA\u8BFB\uFF08D10\uFF09\u2014\u2014 \u9879\u76EE\u7684\u56FE\u53EA\u6709 ideas/graph.yaml \u4E00\u4EFD\u3002\u8981\u5408\u5E76\u65E7\u5185\u5BB9\uFF1Amigrate\u3002` };
  }
  if (relLower === "ideas" || relLower.startsWith("ideas/")) {
    return { allow: true, reason: "\u8D26\u672C\u6587\u4EF6\u53EF\u7F16\u8F91\uFF08status/signed_off \u7684\u9632\u624B\u6539\u7531\u5B88\u536B\u6309\u5177\u4F53\u6539\u52A8\u53E6\u5224\uFF09" };
  }
  const doing = graph.ideas.filter((i) => i.status === "doing" && !isBuildReady(i));
  const needsPlan = (idea) => ({
    allow: false,
    reason: `${idea.id}\u300C${idea.name}\u300D\u7684\u8BA1\u5212\u8FD8\u6CA1\u6709\u5F53\u524D\u6709\u6548\u7684\u4EBA\u5DE5\u6279\u51C6 \u2014\u2014 request-approval --gate plan --node ${idea.id}\uFF0C\u8BF7\u4EBA\u770B\u8FC7\u540E\u6574\u6761\u6D88\u606F\u56DE\u590D\u53E3\u4EE4\uFF08D7\uFF09\u3002`
  });
  const testOwner = doing.find((i) => (i.verify?.test_files ?? []).some((f) => sameFile(f, rel)));
  if (testOwner) {
    if (!validApproval(projectDir, graph, "plan", [testOwner.id])) return needsPlan(testOwner);
    return { allow: true, reason: `${testOwner.id} \u7684\u6D4B\u8BD5\u6587\u4EF6 \u2014\u2014 \u5148\u5199\u4F1A\u5931\u8D25\u7684\u6D4B\u8BD5\u6B63\u662F\u7B2C\u4E00\u6B65` };
  }
  const codeOwner = doing.find((i) => (i.code ?? []).some((c) => c.file && sameFile(c.file, rel)));
  if (codeOwner) {
    if (!validApproval(projectDir, graph, "plan", [codeOwner.id])) return needsPlan(codeOwner);
    const gate = redGateReady(projectDir, graph, codeOwner.id);
    if (!gate.ready) return { allow: false, reason: `\u6D4B\u8BD5\u5148\u884C\uFF08D8\uFF09\uFF1A${gate.reason}` };
    return { allow: true, reason: `${codeOwner.id} \u8BA4\u9886\u4E86\u5B83\uFF0C\u6279\u51C6\u4E0E\u5931\u8D25\u8BB0\u5F55\u4FF1\u5728` };
  }
  return {
    allow: false,
    reason: doing.length === 0 ? "\u6CA1\u6709\u4EFB\u4F55\u60F3\u6CD5\u5728\u8FDB\u884C\u4E2D\uFF0C\u4EA7\u54C1\u6587\u4EF6\u9ED8\u8BA4\u4E0D\u53EF\u5199\uFF08D16\uFF09\u2014\u2014 \u5148 set <id> doing\u3002" : `\u6CA1\u6709\u8FDB\u884C\u4E2D\u7684\u60F3\u6CD5\u8BA4\u9886 ${rel}\uFF08D16\uFF09\u2014\u2014 \u8DEF\u5F84\u7F3A\u53E3\u5E94\u8BE5\u5728\u8BA1\u5212\u91CC\u8865\uFF08code.file / verify.test_files\uFF09\uFF0C\u4E0D\u662F\u5728\u5B9E\u73B0\u65F6\u5F53\u81EA\u7531\u533A\u3002`
  };
}
var isChainedCommand = (command) => /[;&|<>\r\n]|\$\(|`/.test(command);
function chainedCommandRefusal(idea, command) {
  if (!isChainedCommand(command)) return null;
  return `${idea.id}\u300C${idea.name}\u300D\u7684 verify.command \u91CC\u4E32\u4E86\u7B2C\u4E8C\u6761\u547D\u4EE4\uFF08\u5206\u53F7/\u4E0E\u53F7/\u7BA1\u9053/\u91CD\u5B9A\u5411/\u6362\u884C/\u547D\u4EE4\u66FF\u6362\uFF09\u2014\u2014\u300C${command.slice(0, 80)}\u300D\u3002\u9A8C\u8BC1\u547D\u4EE4\u662F\u88AB\u539F\u6837\u4EA4\u7ED9 shell \u8DD1\u7684\uFF0C\u6240\u4EE5\u5B83\u53EA\u80FD\u662F\u4E00\u6761\u547D\u4EE4\uFF08D21/D28\uFF09\uFF1A\u628A\u56FE\u91CC\u8FD9\u6761\u6539\u6210\u5355\u6761\u547D\u4EE4\uFF0C\u591A\u6B65\u9A8C\u8BC1\u62C6\u6210\u591A\u4E2A\u60F3\u6CD5\u6216\u5199\u8FDB\u811A\u672C\u518D\u7531\u4EBA\u8FC7\u76EE\u3002`;
}
function verifyCommandRefusal(projectDir, graph, idea, command) {
  const chained = chainedCommandRefusal(idea, command);
  if (chained) return chained;
  if (!validApproval(projectDir, graph, "plan", [idea.id])) {
    return `${idea.id}\u300C${idea.name}\u300D\u7684\u8BA1\u5212\u6CA1\u6709\u5F53\u524D\u6709\u6548\u7684\u4EBA\u5DE5\u6279\u51C6\uFF0C\u4E0D\u8DD1\u5B83\u7684 verify.command\uFF08D7\uFF09\u2014\u2014\u547D\u4EE4\u662F\u56FE\u91CC\u7684\u6563\u6587\uFF0C\u6CA1\u4EBA\u8FC7\u76EE\u5C31\u7B49\u4E8E\u8BA9 agent \u81EA\u5DF1\u5199\u4E00\u6761\u547D\u4EE4\u518D\u81EA\u5DF1\u6267\u884C\u3002\u5148 \`request-approval --gate plan --node ${idea.id}\`\uFF0C\u8BF7\u4EBA\u56DE\u4E00\u53E5\u300C\u6279\u51C6 CC-\u2026\u300D\uFF1B\u6539\u8FC7 how/code/verify \u4E4B\u540E\u6279\u51C6\u4F1A\u4F5C\u5E9F\uFF0C\u8981\u91CD\u65B0\u8BF7\u3002`;
  }
  return null;
}
function runCheck(projectDir, graph, id, phase, opts = {}) {
  const idea = byId(graph).get(id);
  if (!idea) throw new Error(`no idea with id ${id}`);
  const command = idea.verify?.command;
  if (!command) throw new Error(`${id} \u6CA1\u6709 verify.command \u2014\u2014 \u4EBA\u5DE5\u9A8C\u6536\u7684\u60F3\u6CD5\u7528 manual-check \u5173\u5361`);
  const refusal = verifyCommandRefusal(projectDir, graph, idea, command);
  if (refusal) throw new Error(refusal);
  if (phase === "green") {
    const gate = redGateReady(projectDir, graph, id);
    if (!gate.ready) throw new Error(`\u5148\u8FC7 RED \u95E8\u518D\u8DD1 green\uFF1A${gate.reason}`);
  }
  const missing = missingExecutable(projectDir, command);
  const run = missing ? null : spawnSync(command, {
    shell: true,
    cwd: resolve(projectDir),
    encoding: "utf8",
    timeout: opts.timeoutMs ?? 12e4
  });
  const evidence = readEvidence(projectDir, id);
  const broken = missing ?? infraReason(run);
  const record2 = {
    exit_code: run?.status ?? -1,
    output_tail: `${run?.stdout ?? ""}${run?.stderr ?? ""}${broken ? `
[companion] ${broken}` : ""}`.slice(-2e3),
    test_hashes: hashTests(projectDir, idea),
    at_seq: evidence.change_seq ?? 0
  };
  if (broken) {
    record2.outcome = "infra_error";
    record2.infra_error = broken;
  } else if (phase === "red") {
    record2.outcome = record2.exit_code === 0 ? "unexpected_pass" : "red";
  }
  if (phase === "red") evidence.red = record2;
  else evidence.green = record2;
  writeEvidence(projectDir, id, evidence);
  return record2;
}
function legacyStamp(graph) {
  const g = graph;
  if (!g || typeof g !== "object") return null;
  const agent = typeof g.agent === "string" ? g.agent.trim().toLowerCase() : "";
  const kind = ["claude", "cursor", "codex"].find((k) => k === agent) ?? "cursor";
  if (agent) return { key: `agent: ${agent}`, agent, kind };
  const key = ["enforce", "exempt"].find((k) => g[k] !== void 0);
  return key ? { key: `${key}:`, agent: "cursor", kind } : null;
}
function legacyStampOf(file) {
  try {
    return legacyStamp((0, import_yaml.parseDocument)(readFileSync(file, "utf8")).toJSON());
  } catch {
    return null;
  }
}
function legacyAtCanonical(stamp) {
  return `ideas/graph.yaml \u5E26\u7740 ${stamp.key} \u2014\u2014 \u8FD9\u662F\u65E7\u5B9E\u73B0\uFF08${stamp.agent}\uFF09\u7559\u4E0B\u7684\u56FE\uFF0C\u4E0D\u662F\u672C\u5F15\u64CE\u7684\u9879\u76EE\u56FE\uFF08D10\uFF09\uFF1A\u8BF7\u4EBA\u5148\u624B\u5DE5\u628A\u5B83\u6539\u540D\u6210 ideas/graph.${stamp.agent}.yaml\uFF0C\u518D\u8DD1 \`migrate\` \u628A\u5185\u5BB9\u5E76\u8FDB\u6765\uFF08\u5F15\u64CE\u4E0D\u66FF\u4EBA\u6539\u540D\uFF0C\u4E5F\u4E0D\u52A8\u8FD9\u4E2A\u6587\u4EF6\uFF09\u3002`;
}
function findLegacySources(projectDir) {
  const out = [];
  const plain = graphPath(projectDir);
  if (existsSync(plain)) {
    const stamp = legacyStampOf(plain);
    if (stamp) out.push({ kind: stamp.kind, path: plain, instruction: legacyAtCanonical(stamp) });
  }
  for (const kind of ["claude", "cursor"]) {
    const p = join(IDEAS_DIR(projectDir), `graph.${kind}.yaml`);
    if (existsSync(p)) out.push({ kind, path: p });
  }
  for (const name of [".codex-companion", ".codex-companion.codex"]) {
    const p = join(resolve(projectDir), name, "nodes");
    if (existsSync(p)) {
      out.push({ kind: "codex", path: p });
      break;
    }
  }
  return out;
}
var CODEX_STATUS = {
  draft: "todo",
  aligned: "todo",
  planned: "todo",
  approved: "todo",
  implementing: "doing",
  blocked: "blocked",
  done: "done",
  superseded: "blocked"
};
function convertCodex(nodesDir, projectName, date) {
  const report = [];
  const nodes = readdirSync(nodesDir).filter((f) => f.endsWith(".json")).map((f) => JSON.parse(readFileSync(join(nodesDir, f), "utf8"))).sort((a, b) => `${a.created_at ?? ""}\0${a.id}`.localeCompare(`${b.created_at ?? ""}\0${b.id}`));
  const idMap = new Map(nodes.map((n, i) => [n.id, formatId(i + 1)]));
  for (const [slug, id] of idMap) report.push(`- \u7F16\u53F7\u6620\u5C04\uFF1A${slug} \u2192 ${id}\uFF08D5\uFF1Aslug \u6362\u6210\u987A\u5E8F\u7F16\u53F7\uFF0C\u540D\u5B57\u8FDB name\uFF09`);
  const ideas = nodes.map((n) => {
    const id = idMap.get(n.id);
    const folded = CODEX_STATUS[n.status ?? "draft"] ?? "todo";
    if ((n.status ?? "draft") !== folded) {
      report.push(`- ${id}: \u72B6\u6001 ${n.status} \u6298\u53E0\u4E3A ${folded}\uFF08D4\uFF1A\u516B\u6001\u538B\u56DB\u6001${n.status === "superseded" ? "\uFF1Bsuperseded=\u5E9F\u5F03\uFF0C\u4FDD\u53F7\u7F6E blocked" : ""}\uFF09`);
    }
    const needs = (n.depends_on ?? []).flatMap((slug) => {
      const mapped = idMap.get(slug);
      if (!mapped) report.push(`- ${id}: \u524D\u7F6E ${slug} \u5728\u8282\u70B9\u76EE\u5F55\u91CC\u4E0D\u5B58\u5728\uFF0C\u5DF2\u4E22\u5F03`);
      return mapped ? [mapped] : [];
    });
    if ((n.code_refs ?? []).some((c) => c.role)) {
      report.push(`- ${id}: code_refs.role \u4E0D\u8FDB\u65B0\u683C\u5F0F\uFF0C\u5DF2\u4E22\u5F03\uFF08D2\uFF09`);
    }
    const [first, ...rest] = n.verification ?? [];
    for (const extra of rest) {
      report.push(`- ${id}: \u7B2C\u4E8C\u4E2A\u53CA\u4E4B\u540E\u7684 verification\uFF08${extra.id ?? "?"}\uFF1A${extra.plan ?? ""}\uFF09\u4E0D\u8FDB\u65B0\u683C\u5F0F \u2014\u2014 \u4E00\u4E2A\u60F3\u6CD5\u4E00\u4E2A\u9A8C\u6536\uFF08D2\uFF09`);
    }
    const verify = !first ? void 0 : first.kind === "manual" ? { manual: first.plan ?? "", signed_off: null } : {
      command: Array.isArray(first.command) ? first.command.join(" ") : first.command ?? "",
      test_files: first.test_paths ?? [],
      pass: "exit 0"
    };
    return {
      id,
      name: n.name ?? n.id,
      status: folded,
      ...needs.length ? { needs } : { needs: [] },
      what: n.what ?? "",
      why: n.why ?? "",
      expected: n.expected_result ?? "",
      how: n.implementation?.how ?? "",
      why_this_way: n.implementation?.why_this_way ?? "",
      ...verify ? { verify } : {},
      ...n.code_refs?.length ? { code: n.code_refs.map((c) => ({
        file: c.path,
        ...c.start_line && c.end_line ? { lines: `${c.start_line}-${c.end_line}` } : {}
      })) } : {},
      future: n.future_use ?? "",
      log: [{ date, by: "migrate", note: `\u8FC1\u81EA codex \u8282\u70B9 ${n.id}` }]
    };
  });
  report.push(`- endpoints \u7A7A\u7740\uFF1A\u65E7\u683C\u5F0F\u6CA1\u6709\u7EC8\u70B9\u6982\u5FF5\uFF08D6\uFF09\uFF0C\u9700\u8981\u4EBA\u6765\u5B9A`);
  const graph = { version: 1, project: projectName, next_id: ideas.length + 1, endpoints: [], ideas };
  return { text: (0, import_yaml.stringify)(graph), graph, report };
}
function convertLegacyYaml(text, kind, date) {
  const report = [];
  const doc = (0, import_yaml.parseDocument)(text);
  if (doc.has("agent")) {
    report.push(`- \u53BB\u6389 agent: ${String(doc.get("agent"))} \u952E\uFF08D10\uFF1A\u56FE\u5F52\u9879\u76EE\uFF0C\u4E0D\u5F52 agent\uFF09`);
    doc.delete("agent");
  }
  for (const key of ["enforce", "exempt"]) {
    if (doc.has(key)) {
      report.push(`- \u53BB\u6389 ${key}: \u952E\uFF08D24/D25\uFF1A\u56FE\u5185\u4E0D\u8BBE agent \u53EF\u6539\u7684\u5F00\u5173\uFF09`);
      doc.delete(key);
    }
  }
  const graph = doc.toJSON();
  if (graph.next_id === void 0) {
    const highest = graph.ideas.reduce((m, i) => Math.max(m, idNumber(i.id) || 0), 0);
    writeNextId(doc, highest + 1);
    report.push(`- \u521D\u59CB\u5316\u53D6\u53F7\u8BA1\u6570\u5668 next_id: ${highest + 1}\uFF08\u7528\u8FC7\u7684\u6700\u5927\u7F16\u53F7\u52A0\u4E00\uFF09`);
  }
  report.push(`- \u8FC1\u81EA graph.${kind}.yaml\uFF08${date}\uFF09\uFF0C\u6CE8\u91CA\u539F\u6837\u4FDD\u7559`);
  return { text: String(doc), graph: doc.toJSON(), report };
}
function migrate(projectDir, opts) {
  const plain = graphPath(projectDir);
  const sources = findLegacySources(projectDir);
  const occupied = sources.find((s) => s.instruction);
  if (occupied) return { ok: false, reason: occupied.instruction };
  if (existsSync(plain)) {
    return { ok: false, reason: `ideas/graph.yaml \u5DF2\u5B58\u5728 \u2014\u2014 \u9879\u76EE\u56FE\u5DF2\u5C31\u4F4D\uFF0C\u6CA1\u6709\u53EF\u8FC1\u7684\u4F4D\u7F6E\uFF08\u65E7\u56FE\u4FDD\u6301\u53EA\u8BFB\uFF09` };
  }
  if (sources.length === 0) {
    return { ok: false, reason: "\u6CA1\u6709\u53D1\u73B0\u65E7\u683C\u5F0F\u7684\u56FE\uFF08ideas/graph.claude.yaml / ideas/graph.cursor.yaml / .codex-companion/nodes\uFF09" };
  }
  if (sources.length > 1 && !opts.pick) {
    const lines = sources.map((s) => {
      try {
        const count = s.kind === "codex" ? readdirSync(s.path).filter((f) => f.endsWith(".json")).length : ((0, import_yaml.parseDocument)(readFileSync(s.path, "utf8")).toJSON()?.ideas ?? []).length;
        return `  ${s.kind}: ${count} \u4E2A\u60F3\u6CD5\uFF08${s.path}\uFF09`;
      } catch {
        return `  ${s.kind}: \u8BFB\u4E0D\u51FA\u6765\uFF08${s.path}\uFF09`;
      }
    });
    return {
      ok: false,
      reason: `\u53D1\u73B0\u591A\u4EFD\u65E7\u56FE\uFF0C\u4E0D\u81EA\u52A8\u6311\u8D62\u5BB6\uFF08D10\uFF09\u2014\u2014 \u4EBA\u7528 --pick claude|cursor|codex \u660E\u793A\u9009\u62E9\u6216\u5148\u624B\u5DE5\u5408\u5E76\uFF1A
${lines.join("\n")}`
    };
  }
  const chosen = sources.length === 1 ? sources[0] : sources.find((s) => s.kind === opts.pick);
  if (!chosen) return { ok: false, reason: `--pick ${opts.pick} \u6CA1\u6709\u5BF9\u5E94\u7684\u65E7\u56FE` };
  const projectName = resolve(projectDir).split(/[\\/]/).pop() ?? "project";
  const converted = chosen.kind === "codex" ? convertCodex(chosen.path, projectName, opts.date) : convertLegacyYaml(readFileSync(chosen.path, "utf8"), chosen.kind, opts.date);
  const { errors } = check(converted.graph, projectDir);
  const real = errors.filter((e) => !/code file not found/.test(e));
  if (real.length > 0) {
    return { ok: false, reason: `\u8FC1\u51FA\u6765\u7684\u56FE\u6CA1\u901A\u8FC7\u6821\u9A8C\uFF0C\u4E00\u4E2A\u5B57\u90FD\u6CA1\u5199\uFF1A
${real.map((e) => `  - ${e}`).join("\n")}`, report: converted.report };
  }
  if (opts.dryRun) return { ok: true, report: converted.report };
  mkdirSync(IDEAS_DIR(projectDir), { recursive: true });
  atomicWrite(plain, converted.text);
  atomicWrite(
    join(IDEAS_DIR(projectDir), "migrate-report.md"),
    `# \u8FC1\u79FB\u62A5\u544A\uFF08${opts.date}\uFF0C\u6765\u6E90\uFF1A${chosen.kind}\uFF09

\u6CA1\u80FD\u65E0\u635F\u8F6C\u6362\u7684\u5185\u5BB9\uFF0C\u9010\u6761\u5217\u5728\u8FD9\u91CC\uFF1A

${converted.report.join("\n")}
`
  );
  return { ok: true, written: plain, report: converted.report };
}
var TRANSITIONS = {
  todo: ["doing", "blocked"],
  doing: ["done", "blocked"],
  blocked: ["todo", "doing"],
  done: ["blocked"]
  // a regression reopens it; nothing else moves done
};
function setStatus(doc, graph, id, status, entry, projectDir) {
  const index = graph.ideas.findIndex((i) => i.id === id);
  if (index < 0) throw new Error(`no idea with id ${id}`);
  if (!STATUSES.includes(status)) throw new Error(`status must be one of ${STATUSES.join(" | ")}`);
  const idea = graph.ideas[index];
  const from = idea.status ?? "todo";
  if (!TRANSITIONS[from].includes(status)) {
    throw new Error(`${id}: ${from} \u2192 ${status} \u4E0D\u5728\u8F6C\u79FB\u8868\u91CC\uFF08todo\u2192doing|blocked, doing\u2192done|blocked, blocked\u2192todo|doing, done\u2192blocked\uFF09`);
  }
  if (status === "doing") {
    const notReady = isBuildReady(idea);
    if (notReady) throw new Error(`${id}: cannot be doing \u2014 ${notReady}. \u5148\u628A\u60F3\u6CD5\u60F3\u6E05\u695A\uFF08/ccthink\uFF09`);
    const unmet = needsUnmet(idea, graph);
    if (unmet) throw new Error(`${id}: cannot be doing \u2014 ${unmet}`);
    const clash = fileClash(idea, graph);
    if (clash) throw new Error(`${id}: cannot be doing \u2014 ${clash}`);
    if (projectDir) {
      if (!validApproval(projectDir, graph, "decomposition")) {
        throw new Error(`${id}: cannot be doing \u2014 \u62C6\u5206\u8FD8\u6CA1\u6709\u5F53\u524D\u6709\u6548\u7684\u4EBA\u5DE5\u6279\u51C6 \u2014\u2014 request-approval --gate decomposition\uFF0C\u8BF7\u4EBA\u770B\u8FC7\u6574\u5F20\u56FE\u7684\u540D\u79F0\u3001\u8FB9\u548C\u524D\u4E09\u95EE\uFF0C\u518D\u6574\u6761\u6D88\u606F\u56DE\u590D\u53E3\u4EE4\uFF08D7/D17\uFF09`);
      }
      if (!validApproval(projectDir, graph, "plan", [id])) {
        throw new Error(`${id}: cannot be doing \u2014 \u8BA1\u5212\u8FD8\u6CA1\u6709\u5F53\u524D\u6709\u6548\u7684\u4EBA\u5DE5\u6279\u51C6 \u2014\u2014 request-approval --gate plan --node ${id}\uFF0C\u8BF7\u4EBA\u770B\u8FC7\u8FD9\u4E2A\u60F3\u6CD5\u7684\u516B\u95EE\uFF0C\u518D\u6574\u6761\u6D88\u606F\u56DE\u590D\u53E3\u4EE4\uFF08D7/D17\uFF09`);
      }
    }
  }
  if (status === "done") {
    if (!idea.code?.length) throw new Error(`${id}: cannot be done without \`code\` \u2014 say where it lives`);
    if (!idea.verify) throw new Error(`${id}: cannot be done without \`verify\``);
    if (idea.verify.manual && !idea.verify.signed_off) {
      throw new Error(`${id}: manual check \u2014 a human must fill \`verify.signed_off\` before done`);
    }
    if (projectDir && idea.verify.command && !greenCurrent(projectDir, graph, id)) {
      throw new Error(`${id}: \u5B8C\u6210\u524D\u5FC5\u987B\u6709\u5F53\u524D\u6709\u6548\u7684 GREEN \u2014\u2014 run-check ${id} --phase green\uFF08\u5B9E\u73B0\u6BCF\u6539\u4E00\u6B21\u3001\u6D4B\u8BD5\u6BCF\u53D8\u4E00\u6B21\u90FD\u8981\u91CD\u8DD1\uFF09`);
    }
  }
  doc.setIn(["ideas", index, "status"], status);
  const log = (idea.log ?? []).concat({
    date: entry.date,
    ...entry.by ? { by: entry.by } : {},
    note: entry.note || `status \u2192 ${status}`
  });
  doc.setIn(["ideas", index, "log"], log);
}
var CHANGE_VERSION = 1;
var BEHAVIOUR_FIELDS = ["what", "expected", "how", "why_this_way", "verify"];
var NEW_IDEA_FIELDS = ["name", "what", "why", "expected", "how", "why_this_way", "future"];
var SET_FIELDS = ["name", "what", "why", "expected", "how", "why_this_way", "future"];
var idNumber = (id) => {
  const m = /^I-(\d+)$/.exec(String(id));
  return m ? Number(m[1]) : NaN;
};
var formatId = (n) => `I-${String(n).padStart(3, "0")}`;
function proseNode(doc, value) {
  const node = doc.createNode(String(value ?? ""));
  node.type = "BLOCK_FOLDED";
  return node;
}
function needsNode(doc, ids) {
  const node = doc.createNode(ids);
  node.flow = true;
  return node;
}
function applyChanges(source, envelope, today, projectDir) {
  const env2 = envelope;
  if (!env2 || env2.v !== CHANGE_VERSION) {
    return { ok: false, reason: `\u4E0D\u8BA4\u8BC6\u7684\u6539\u52A8\u683C\u5F0F\u7248\u672C\uFF1A${env2?.v} \u2014\u2014 \u6574\u4F53\u62D2\u7EDD` };
  }
  const current = fingerprint(source);
  if (env2.baseDigest !== current) {
    return {
      ok: false,
      reason: `\u60F3\u6CD5\u56FE\u5728\u8FD9\u4EFD\u6539\u52A8\u5199\u6210\u4E4B\u540E\u88AB\u6539\u8FC7\u4E86\uFF08\u6539\u52A8\u57FA\u4E8E ${env2.baseDigest}\uFF0C\u73B0\u5728\u662F ${current}\uFF09\uFF0C\u6574\u4F53\u62D2\u7EDD`
    };
  }
  const ops = (env2.ops ?? []).map((o) => ({ ...o }));
  const doc = (0, import_yaml.parseDocument)(source);
  if (doc.errors.length > 0) return { ok: false, reason: `\u56FE\u672C\u8EAB\u5C31\u6709\u8BED\u6CD5\u9519\u8BEF\uFF1A${doc.errors[0].message}` };
  const before = doc.toJSON();
  const highest = before.ideas.reduce((m, i) => Math.max(m, idNumber(i.id) || 0), 0);
  let nextId = Number(before.next_id) || highest + 1;
  const real = /* @__PURE__ */ new Map();
  for (const op of ops) {
    if (op.op !== "add" || !op.tmp) continue;
    real.set(op.tmp, formatId(nextId));
    nextId += 1;
  }
  const resolve3 = (v) => v && real.get(v) || v;
  for (const op of ops) {
    for (const key of ["id", "tmp", "from", "to"]) {
      if (op[key] !== void 0) op[key] = resolve3(op[key]);
    }
  }
  const leftover = ops.find((o) => ["id", "tmp", "from", "to"].some((k) => typeof o[k] === "string" && o[k].startsWith("tmp:")));
  if (leftover) {
    return { ok: false, reason: `\u6539\u52A8\u91CC\u8FD8\u5269\u6CA1\u6709\u53D1\u5230\u7F16\u53F7\u7684\u4E34\u65F6\u53F7\uFF08${leftover.op} \u4E0A\u7684 ${["id", "tmp", "from", "to"].map((k) => o_(leftover, k)).find((v) => v)}\uFF09\uFF0C\u6574\u4F53\u62D2\u7EDD` };
  }
  const changed = [];
  const indexOf = (id) => doc.toJSON().ideas.findIndex((i) => i.id === id);
  for (const op of ops) {
    if (op.op !== "add") continue;
    const fields = op.fields ?? {};
    const node = doc.createNode({});
    node.set("id", op.tmp);
    node.set("name", String(fields.name ?? ""));
    node.set("status", "todo");
    node.set("needs", needsNode(doc, []));
    for (const f of NEW_IDEA_FIELDS) {
      if (f === "name" || fields[f] === void 0) continue;
      node.set(f, proseNode(doc, fields[f]));
    }
    doc.addIn(["ideas"], node);
    changed.push(`\u65B0\u5EFA ${op.tmp}\u300C${String(fields.name ?? "")}\u300D`);
  }
  for (const op of ops) {
    if (op.op !== "set" && op.op !== "status") continue;
    const index = indexOf(op.id);
    if (index < 0) return { ok: false, reason: `\u6539\u52A8\u6307\u5411\u4E0D\u5B58\u5728\u7684\u60F3\u6CD5 ${op.id}` };
    const idea = doc.toJSON().ideas[index];
    if (op.op === "status") {
      if (op.to === "done") {
        return {
          ok: false,
          reason: `${op.id}: \u7F51\u9875\u6539\u4E0D\u51FA done \u2014\u2014 \u5B8C\u6210\u8981\u6709\u5F53\u524D\u6709\u6548\u7684 GREEN \u8BC1\u636E\uFF08D20\uFF09\uFF1A\u5148 run-check ${op.id} --phase green\uFF0C\u518D set ${op.id} done\uFF0C\u6574\u4F53\u62D2\u7EDD`
        };
      }
      if (op.to === "doing" && !projectDir) {
        return {
          ok: false,
          reason: `${op.id}: \u8FD9\u6B21\u5199\u56DE\u4E0D\u77E5\u9053\u9879\u76EE\u76EE\u5F55\u5728\u54EA\u513F\uFF0C\u8BFB\u4E0D\u5230\u6279\u51C6\u56DE\u6267\uFF0Cdoing \u4E00\u5F8B\u4E0D\u7ED9\uFF08D17\uFF09\uFF1A\u6539\u6210 apply --project <\u9879\u76EE\u76EE\u5F55>\uFF0C\u6216\u8005\u8D70\u547D\u4EE4\u884C set ${op.id} doing\uFF0C\u6574\u4F53\u62D2\u7EDD`
        };
      }
      try {
        setStatus(
          doc,
          doc.toJSON(),
          op.id,
          op.to,
          { by: "apply", note: `\u7F51\u9875\u4E0A\u6539\u7684\u72B6\u6001\uFF1A${op.from} \u2192 ${op.to}`, date: today },
          projectDir
        );
      } catch (error) {
        return { ok: false, reason: String(error instanceof Error ? error.message : error) };
      }
      changed.push(`${op.id} \u72B6\u6001 ${op.from} \u2192 ${op.to}`);
      continue;
    }
    if (!SET_FIELDS.includes(op.field)) {
      return {
        ok: false,
        reason: `\u6539\u52A8\u60F3\u6539 ${op.id} \u7684 ${op.field} \u2014\u2014 \u5199\u56DE\u53EA\u8BA4\u8FD9\u51E0\u4E2A\u5B57\u6BB5\uFF1A${SET_FIELDS.join("\u3001")}\uFF1B\u72B6\u6001\u3001\u9A8C\u8BC1\u65B9\u5F0F\u3001\u7B7E\u5B57\u8FD9\u4E9B\u53EA\u80FD\u8D70\u547D\u4EE4\u884C\uFF08D24\uFF09\uFF0C\u6574\u4F53\u62D2\u7EDD`
      };
    }
    doc.setIn(["ideas", index, op.field], proseNode(doc, op.new));
    changed.push(`${op.id} \xB7 ${op.field}`);
    if (idea.status === "done" && BEHAVIOUR_FIELDS.includes(op.field)) {
      setStatus(doc, doc.toJSON(), op.id, "blocked", {
        by: "apply",
        date: today,
        note: `\u5DF2\u5B8C\u6210\u7684\u60F3\u6CD5\u88AB\u6539\u4E86 ${op.field}\uFF0C\u81EA\u52A8\u9000\u56DE blocked \u2014\u2014 \u60F3\u6E05\u695A\u518D\u8D70\u4E00\u904D doing\uFF0C\u6D4B\u8BD5\u5148\u884C\u3001\u4EBA\u6279\u51C6\u4E09\u6761\u89C4\u5219\u5BF9\u5B83\u91CD\u65B0\u751F\u6548`
      }, projectDir);
      changed.push(`${op.id} \u56E0\u884C\u4E3A\u5B57\u6BB5\u88AB\u6539\uFF0C\u9000\u56DE blocked`);
    }
  }
  const signRequests = [];
  for (const op of ops) {
    if (op.op !== "sign") continue;
    const index = indexOf(op.id);
    if (index < 0) return { ok: false, reason: `\u8981\u7B7E\u5B57\u7684\u60F3\u6CD5 ${op.id} \u4E0D\u5B58\u5728` };
    const idea = doc.toJSON().ideas[index];
    if (!idea.verify?.manual) {
      return { ok: false, reason: `${op.id} \u7684\u9A8C\u8BC1\u4E0D\u662F\u4EBA\u5DE5\u68C0\u67E5\uFF0C\u7B7E\u5B57\u5BF9\u5B83\u6CA1\u6709\u610F\u4E49` };
    }
    if (idea.verify.signed_off) {
      return {
        ok: false,
        reason: `${op.id} \u5DF2\u7ECF\u7B7E\u8FC7\u5B57\u4E86\uFF08${idea.verify.signed_off}\uFF09\u2014\u2014 \u8C01\u80FD\u5728\u4EC0\u4E48\u60C5\u51B5\u4E0B\u63A8\u7FFB\u522B\u4EBA\u7684\u7B7E\u5B57\uFF0C\u662F\u4E00\u4EF6\u8FD8\u6CA1\u60F3\u6E05\u695A\u7684\u4E8B\uFF0C\u8FD9\u91CC\u5148\u4E0D\u8986\u76D6`
      };
    }
    const who = String(op.who ?? "").trim();
    const words = String(op.words ?? "").trim();
    if (!who) return { ok: false, reason: `${op.id} \u7684\u7B7E\u5B57\u6CA1\u6709\u540D\u5B57 \u2014\u2014 \u67E5\u4E0D\u5230\u662F\u8C01\u7B7E\u7684\u8BB0\u5F55\u6CA1\u6709\u610F\u4E49` };
    if (!words) return { ok: false, reason: `${op.id} \u7684\u7B7E\u5B57\u6CA1\u6709\u539F\u8BDD \u2014\u2014 \u7A7A\u767D\u7684\u7B7E\u540D\u7B49\u4E8E\u6CA1\u7B7E` };
    signRequests.push({ id: op.id, who, words });
    changed.push(`${op.id} \u8BF7\u6C42\u4EBA\u5DE5\u9A8C\u8BC1\u7B7E\u5B57\uFF08${who}\uFF09\u2014\u2014 \u8FD8\u6CA1\u5199\u8FDB\u56FE\uFF0C\u7B49\u4EBA\u56DE\u4E00\u6B21\u6027\u53E3\u4EE4`);
  }
  for (const op of ops) {
    if (op.op !== "link" && op.op !== "unlink") continue;
    const index = indexOf(op.to);
    if (index < 0) return { ok: false, reason: `\u6539\u52A8\u7ED9\u4E0D\u5B58\u5728\u7684\u60F3\u6CD5 ${op.to} \u8FDE\u8FB9` };
    const needs = (doc.toJSON().ideas[index].needs ?? []).slice();
    const next = op.op === "link" ? needs.includes(op.from) ? needs : needs.concat(op.from) : needs.filter((n) => n !== op.from);
    doc.setIn(["ideas", index, "needs"], needsNode(doc, next));
    changed.push(`${op.to} ${op.op === "link" ? "\u52A0\u4E0A" : "\u53BB\u6389"}\u524D\u7F6E ${op.from}`);
  }
  for (const op of ops) {
    if (op.op !== "remove") continue;
    const graph = doc.toJSON();
    const index = graph.ideas.findIndex((i) => i.id === op.id);
    if (index < 0) return { ok: false, reason: `\u8981\u5220\u7684\u60F3\u6CD5 ${op.id} \u4E0D\u5B58\u5728` };
    const dependents2 = graph.ideas.filter((i) => (i.needs ?? []).includes(op.id)).map((i) => i.id);
    if (dependents2.length > 0) {
      return { ok: false, reason: `${op.id} \u8FD8\u88AB ${dependents2.join("\u3001")} \u4F9D\u8D56\u7740\uFF0C\u4E0D\u80FD\u5220 \u2014\u2014 \u5148\u65AD\u5F00\u90A3\u4E9B\u524D\u7F6E` };
    }
    const seq = doc.getIn(["ideas"]);
    const doomed = seq.items[index];
    const carried = index === 0 ? seq.commentBefore : doomed?.commentBefore;
    doc.deleteIn(["ideas", index]);
    if (carried) {
      if (index === 0) seq.commentBefore = carried;
      else if (seq.items[index]) seq.items[index].commentBefore = carried;
    }
    changed.push(`\u5220\u6389 ${op.id}`);
  }
  if (real.size > 0 || before.next_id !== void 0) {
    writeNextId(doc, nextId);
  }
  const after = doc.toJSON();
  if (!after || !Array.isArray(after.ideas)) return { ok: false, reason: "\u5E94\u7528\u4E4B\u540E\u7684\u56FE\u8BFB\u4E0D\u51FA\u6765\u4E86" };
  const { errors } = check(after, ".");
  const real_errors = errors.filter((e) => !/code file not found/.test(e));
  if (real_errors.length > 0) {
    return { ok: false, reason: `\u5E94\u7528\u4E4B\u540E\u56FE\u6821\u9A8C\u4E0D\u8FC7\uFF0C\u6574\u4F53\u653E\u5F03\uFF1A
  - ${real_errors.join("\n  - ")}` };
  }
  return { ok: true, text: String(doc), changed, signRequests };
}
function requestSignatures(projectDir, graph, requests, date) {
  return requests.map((r) => {
    try {
      const { challenge } = requestApproval(projectDir, graph, "manual-check", [r.id], { by: r.who, date });
      return `${r.id} \u7684\u4EBA\u5DE5\u9A8C\u8BC1\u8981\u4EBA\u4EB2\u53E3\u7B7E\uFF1A\u6574\u6761\u6D88\u606F\u56DE\u4E00\u53E5\u300C\u6279\u51C6 ${challenge}\u300D\uFF0C\u7B7E\u5B57\u624D\u4F1A\u5199\u8FDB\u56FE\uFF08\u7F51\u9875\u4E0A\u5199\u7684\u539F\u8BDD\uFF1A\u300C${r.words}\u300D\uFF09`;
    } catch (error) {
      return `${r.id} \u7684\u7B7E\u5B57\u8BF7\u6C42\u6CA1\u53D1\u51FA\u53BB\uFF1A${error instanceof Error ? error.message : String(error)}`;
    }
  });
}
var o_ = (op, key) => typeof op[key] === "string" && op[key].startsWith("tmp:") ? op[key] : void 0;
var esc = (s = "") => String(s ?? "").replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
var attr = (s = "") => esc(s).replace(/"/g, "&quot;");
var fingerprint = (text) => createHash("sha256").update(text.replaceAll("\r\n", "\n")).digest("hex").slice(0, 12);
var NONE = "<span class='none'>\u2014</span>";
var QUESTIONS = [
  { n: 1, key: "what", prose: true, label: "\u662F\u4EC0\u4E48" },
  { n: 2, key: "why", prose: true, label: "\u4E3A\u4EC0\u4E48\u6709\u8FD9\u4E2A\u60F3\u6CD5" },
  { n: 3, key: "expected", prose: true, label: "\u9884\u671F\u7ED3\u679C" },
  { n: 4, key: "how", prose: true, label: "\u5982\u4F55\u5B9E\u73B0" },
  { n: 5, key: "why_this_way", prose: true, label: "\u4E3A\u4EC0\u4E48\u8FD9\u6837\u5B9E\u73B0" },
  { n: 6, key: "code", prose: false, label: "\u4EE3\u7801\u5728\u54EA" },
  { n: 7, key: "verify", prose: false, label: "\u5982\u4F55\u9A8C\u8BC1" },
  { n: 8, key: "future", prose: true, label: "\u672A\u6765\u600E\u4E48\u7528" }
];
var askedAs = (n) => QUESTIONS[n - 1].label;
var PROSE = QUESTIONS.filter((q) => q.prose);
var MERMAID_SOURCE_FN = `function buildMermaidSource(g) {
  var ideas = (g && g.ideas) || [];
  var present = new Set(ideas.map(function (i) { return i.id; }));
  var ends = new Set((g && g.endpoints) || []);
  var mid = function (id) { return "n_" + String(id).replace(/[^A-Za-z0-9]/g, "_"); };
  var cls = function (i) { return ends.has(i.id) ? "endpoint" : (i.status || "todo"); };
  var wrap = function (name, max) {
    max = max || 12;
    if (Array.from(name).length <= max) return name;
    var units = name.match(/[\\u3000-\\u9fff\\uf900-\\ufaff\\uff00-\\uffef]|[^\\s\\u3000-\\u9fff\\uf900-\\ufaff\\uff00-\\uffef]+|\\s+/g) || [name];
    var lines = [], line = "";
    for (var u = 0; u < units.length; u++) {
      if (line && Array.from(line + units[u]).length > max) { lines.push(line.trim()); line = ""; }
      line += units[u];
    }
    if (line.trim()) lines.push(line.trim());
    return lines.join("<br>");
  };
  var out = [
    "flowchart TD",
    "classDef done fill:#14532d,stroke:#86efac,color:#f0fdf4;",
    "classDef doing fill:#1e3a8a,stroke:#93c5fd,color:#eff6ff;",
    "classDef todo fill:#334155,stroke:#94a3b8,color:#f1f5f9,stroke-dasharray:5 3;",
    "classDef blocked fill:#7c2d12,stroke:#fdba74,color:#fff7ed,stroke-dasharray:2 2;",
    "classDef endpoint fill:#581c87,stroke:#d8b4fe,color:#faf5ff,stroke-width:3px;"
  ];
  for (var a = 0; a < ideas.length; a++) {
    out.push(mid(ideas[a].id) + '["' + wrap(String(ideas[a].name || ideas[a].id).replace(/["()<>]/g, "")) + '"]');
  }
  for (var b = 0; b < ideas.length; b++) {
    var needs = ideas[b].needs || [];
    for (var c = 0; c < needs.length; c++) {
      if (present.has(needs[c])) out.push(mid(needs[c]) + " --> " + mid(ideas[b].id));
    }
  }
  var buckets = {};
  for (var d = 0; d < ideas.length; d++) {
    var k = cls(ideas[d]);
    (buckets[k] = buckets[k] || []).push(mid(ideas[d].id));
  }
  var keys = Object.keys(buckets);
  for (var e = 0; e < keys.length; e++) out.push("class " + buckets[keys[e]].join(",") + " " + keys[e] + ";");
  for (var f = 0; f < ideas.length; f++) {
    out.push("click " + mid(ideas[f].id) + ' call nodeClick("' + ideas[f].id + '")');
  }
  return out.join("\\n");
}`;
var buildMermaidSource = new Function(`${MERMAID_SOURCE_FN}
return buildMermaidSource;`)();
var LEDGER_FN = `function createLedger(graph, baseDigest, project) {
  var VERSION = 1;
  var ideas = (graph && graph.ideas) || [];
  var original = {};
  for (var i = 0; i < ideas.length; i++) original[ideas[i].id] = ideas[i];

  var ops = [];
  var seq = 0;
  var isTmp = function (id) { return String(id).indexOf("tmp:") === 0; };
  var drop = function (pred) { ops = ops.filter(function (o) { return !pred(o); }); };
  var origField = function (id, field) {
    var idea = original[id];
    var v = idea ? idea[field] : undefined;
    return v === undefined || v === null ? "" : v;
  };
  var origNeeds = function (id) {
    var idea = original[id];
    return (idea && idea.needs) || [];
  };
  var hasEdge = function (from, to) { return origNeeds(to).indexOf(from) >= 0; };

  return {
    setField: function (id, field, value) {
      drop(function (o) { return o.op === "set" && o.id === id && o.field === field; });
      var old = origField(id, field);
      if (String(old) !== String(value)) ops.push({ op: "set", id: id, field: field, old: old, new: value });
    },
    setStatus: function (id, value) {
      drop(function (o) { return o.op === "status" && o.id === id; });
      var from = (original[id] && original[id].status) || "todo";
      if (from !== value) ops.push({ op: "status", id: id, from: from, to: value });
    },
    addIdea: function (fields) {
      var tmp = "tmp:" + (++seq);
      ops.push({ op: "add", tmp: tmp, fields: fields });
      return tmp;
    },
    removeIdea: function (id) {
      // A brand-new idea that never reached disk: drop it and everything that
      // referred to it, rather than handing the write-back two cancelling orders.
      if (isTmp(id)) {
        // Every kind, not most kinds: one op left behind carries a tmp id nobody
        // owns, and "any leftover tmp id rejects the whole file" is the
        // write-back's rule \u2014 so one forgotten draft would sink every other
        // edit submitted with it.
        drop(function (o) {
          return (o.op === "add" && o.tmp === id) ||
                 (o.op === "set" && o.id === id) ||
                 (o.op === "status" && o.id === id) ||
                 ((o.op === "link" || o.op === "unlink") && (o.from === id || o.to === id));
        });
        return;
      }
      drop(function (o) { return o.op === "remove" && o.id === id; });
      ops.push({ op: "remove", id: id });
    },
    link: function (from, to) {
      var pending = ops.some(function (o) { return o.op === "unlink" && o.from === from && o.to === to; });
      if (pending) { drop(function (o) { return o.op === "unlink" && o.from === from && o.to === to; }); return; }
      if (hasEdge(from, to)) return;
      if (ops.some(function (o) { return o.op === "link" && o.from === from && o.to === to; })) return;
      ops.push({ op: "link", from: from, to: to });
    },
    /** A person's own words, standing behind a check only a person can make. */
    sign: function (id, who, words) {
      if (!String(who || "").trim() || !String(words || "").trim()) return false;
      drop(function (o) { return o.op === "sign" && o.id === id; });
      ops.push({ op: "sign", id: id, who: String(who).trim(), words: String(words).trim() });
      return true;
    },
    unlink: function (from, to) {
      var pending = ops.some(function (o) { return o.op === "link" && o.from === from && o.to === to; });
      if (pending) { drop(function (o) { return o.op === "link" && o.from === from && o.to === to; }); return; }
      if (!hasEdge(from, to)) return;
      if (ops.some(function (o) { return o.op === "unlink" && o.from === from && o.to === to; })) return;
      ops.push({ op: "unlink", from: from, to: to });
    },
    /** What this idea's prerequisites look like with the pending edits applied. */
    needsOf: function (id) {
      var out = origNeeds(id).slice();
      for (var k = 0; k < ops.length; k++) {
        var o = ops[k];
        if (o.op === "unlink" && o.to === id) out = out.filter(function (n) { return n !== o.from; });
        if (o.op === "link" && o.to === id && out.indexOf(o.from) < 0) out.push(o.from);
      }
      return out;
    },
    ops: function () { return ops.slice(); },
    isEmpty: function () { return ops.length === 0; },
    envelope: function () {
      // baseDigest is carried through untouched \u2014 the page never computes it.
      return { v: VERSION, project: project, baseDigest: baseDigest, ops: ops.slice() };
    },
    load: function (env) {
      if (!env || env.v !== VERSION) throw new Error("\u4E0D\u8BA4\u8BC6\u7684\u6539\u52A8\u683C\u5F0F\u7248\u672C\uFF1A" + (env && env.v));
      ops = (env.ops || []).slice();
      for (var m = 0; m < ops.length; m++) {
        if (ops[m].op === "add") seq = Math.max(seq, Number(String(ops[m].tmp).slice(4)) || 0);
      }
      // Stale is reported, never decided here: a draft written against an older
      // graph is the caller's problem to surface, not this book's to discard.
      return { stale: env.baseDigest !== baseDigest, count: ops.length };
    },
  };
}`;
var createLedger = new Function(`${LEDGER_FN}
return createLedger;`)();
function render(g, source = "", projectDir = "", token = "") {
  const map = byId(g);
  const ends = new Set(g.endpoints ?? []);
  const cls = (i) => ends.has(i.id) ? "endpoint" : i.status ?? "todo";
  const mermaid = buildMermaidSource(g);
  const links = (ids) => ids.length === 0 ? NONE : ids.map((id) => `<a class="xlink" href="#${esc(id)}" data-goto="${esc(id)}">${esc(map.get(id)?.name ?? id)}</a>`).join(" ");
  const needChip = (from, to) => `<span class="chip"><a class="xlink" href="#${esc(from)}" data-goto="${esc(from)}">${esc(map.get(from)?.name ?? from)}</a><button class="cut" title="\u65AD\u5F00\u8FD9\u6761\u524D\u7F6E" data-unlink-from="${attr(from)}" data-unlink-to="${attr(to)}">\xD7</button></span>`;
  const linkPicker = (i) => `<select class="rw-edge" data-link-to="${attr(i.id)}">
    <option value="">\uFF0B \u8FDE\u4E00\u6761\u524D\u7F6E\u2026</option>${g.ideas.filter((o) => o.id !== i.id && !(i.needs ?? []).includes(o.id)).map((o) => `<option value="${attr(o.id)}">${esc(o.name || o.id)}</option>`).join("")}</select>`;
  const codeOf = (i) => !i.code?.length ? "<span class='none'>\u5C1A\u672A\u5B9E\u73B0</span>" : i.code.map((c) => `<code>${esc(c.file)}${c.lines ? ":" + esc(c.lines) : ""}</code>${c.symbol ? ` \xB7 ${esc(c.symbol)}` : ""}`).join("<br>");
  const verifyOf = (i) => {
    const v = i.verify;
    if (!v) return NONE;
    if (v.command) return `<code>${esc(v.command)}</code>${v.pass ? ` \u2192 ${esc(v.pass)}` : ""}`;
    const sign = v.signed_off ? "" : `<button class="sign-open" data-sign="${attr(i.id)}" data-manual="${attr(v.manual)}">\u4EBA\u5DE5\u7B7E\u5B57</button>`;
    return `${esc(v.manual)}<br><span class="signoff">\u4EBA\u5DE5\u7B7E\u5B57\uFF1A${v.signed_off ? esc(v.signed_off) : "\u672A\u7B7E"}</span>${sign}`;
  };
  const awaitingSignature = (i) => !!i.verify && !i.verify.command && !i.verify.signed_off;
  const waitingOn = (i) => (i.needs ?? []).filter((n) => map.has(n) && map.get(n).status !== "done");
  const worklist = (title, rows) => rows.length === 0 ? "" : `<details class="worklist"><summary>${esc(title)} (${rows.length})</summary>
  ${rows.map((r) => `<div class="wl-row"><a class="xlink" href="#${esc(r.id)}" data-goto="${esc(r.id)}">${esc(r.name)}</a><span class="wl-note">${esc(r.note)}</span></div>`).join("\n  ")}
</details>`;
  const STATUS_ZH = { todo: "\u5F85\u529E", doing: "\u8FDB\u884C\u4E2D", done: "\u5DF2\u5B8C\u6210", blocked: "\u53D7\u963B" };
  const counts = STATUSES.map((s) => `${STATUS_ZH[s]} ${g.ideas.filter((i) => (i.status ?? "todo") === s).length}`).join(" \xB7 ");
  const field = (i, name, label) => `
    <dt>${label}</dt><dd data-f="${name}"><span class="ro">${esc(i[name]) || NONE}</span
      ><textarea class="rw" data-idea="${attr(i.id)}" data-field="${name}" rows="3">${esc(i[name] ?? "")}</textarea></dd>`;
  const statusPicker = (i) => `<select class="rw" data-idea="${attr(i.id)}" data-field="status">${STATUSES.map((s) => `<option value="${s}"${(i.status ?? "todo") === s ? " selected" : ""}>${STATUS_ZH[s]}</option>`).join("")}</select>`;
  const card = (i) => `<section class="idea ${cls(i)}" id="${esc(i.id)}">
  <h3><span class="ro">${esc(i.name)}</span><input class="rw" data-idea="${attr(i.id)}" data-field="name" value="${attr(i.name)}"> <span class="badge ro">${esc(STATUS_ZH[i.status ?? "todo"])}</span>${statusPicker(i)}${ends.has(i.id) ? '<span class="badge end">\u7EC8\u70B9</span>' : ""}<button class="edit-toggle" data-edit="${attr(i.id)}">\u7F16\u8F91</button><button class="edit-toggle danger" data-remove="${attr(i.id)}" title="\u6807\u8BB0\u5F85\u5220\uFF0C\u518D\u70B9\u4E00\u6B21\u64A4\u9500">\u5220\u9664</button><span class="iid">${esc(i.id)}</span></h3>
  <dl>${PROSE.slice(0, 5).map((q) => field(i, q.key, q.label)).join("")}
    <dt>${askedAs(6)}</dt><dd>${codeOf(i)}</dd>
    <dt>${askedAs(7)}</dt><dd>${verifyOf(i)}</dd>${field(i, "future", askedAs(8))}
  </dl>
  <p class="edges needs" data-needs-of="${attr(i.id)}"><b>\u524D\u7F6E\u60F3\u6CD5</b> <span class="chips">${(i.needs ?? []).filter((n) => map.has(n)).map((n) => needChip(n, i.id)).join("") || NONE}</span>${linkPicker(i)}</p>
  <p class="edges"><b>\u5B83\u662F\u8FD9\u4E9B\u60F3\u6CD5\u7684\u524D\u7F6E</b> ${links(dependents(g, i.id))}</p>
  ${i.log?.length ? `<details class="log"><summary>\u4FEE\u6539\u8BB0\u5F55 (${i.log.length})</summary>${i.log.map((l) => `<div>${esc(l.date)}${l.by ? " \xB7 " + esc(l.by) : ""} \u2014 ${esc(l.note)}</div>`).join("")}</details>` : ""}
</section>`;
  return `<!doctype html><html lang="zh"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>${esc(g.project ?? "idea graph")} \u2014 \u60F3\u6CD5\u56FE</title>
<style>
  :root { color-scheme: dark; }
  * { box-sizing: border-box; }
  body { font:15px/1.6 system-ui,-apple-system,"Segoe UI",sans-serif; margin:0 auto; max-width:1060px;
    padding:28px 22px 80px; background:#0b0f14; color:#e6edf3; }
  h1 { margin:0 0 6px; font-size:22px; }
  .overview { color:#93a1b0; margin:0 0 18px; }
  .legend { font-size:13px; color:#7d8896; margin:0 0 4px; }
  .sw { display:inline-block; width:11px; height:11px; border-radius:3px; vertical-align:-1px; margin:0 5px 0 12px; }
  .sw:first-child { margin-left:0; }
  .graph { background:#0d1117; border:1px solid #1f2933; border-radius:10px; margin:14px 0 26px; position:relative; }
  /* Pan/zoom: the viewport clips, the canvas is what gets transformed. */
  .viewport { overflow:hidden; height:min(72vh,760px); touch-action:none; cursor:grab; border-radius:10px; }
  .viewport.dragging { cursor:grabbing; }
  /* Only ever translated \u2014 never scaled. A CSS scale() would rasterise the
     layer once and stretch that bitmap, which is exactly what looks blurry. */
  .canvas { transform-origin:0 0; will-change:transform; display:inline-block; padding:0; line-height:0; }
  .canvas svg { max-width:none !important; display:block; }
  .graph-tools { position:absolute; top:10px; right:10px; z-index:2; display:flex; gap:4px; align-items:center;
    background:#0d1117cc; border:1px solid #1f2933; border-radius:8px; padding:4px 6px; backdrop-filter:blur(4px); }
  .graph-tools button { width:26px; height:24px; font-size:13px; line-height:1; cursor:pointer;
    background:#161b22; color:#c3ced9; border:1px solid #232c36; border-radius:5px; padding:0; }
  .graph-tools button:hover { border-color:#7dd3fc; color:#7dd3fc; }
  .graph-tools button.wide { width:auto; padding:0 8px; font-size:12px; }
  .zoom-level { font-size:11px; color:#7d8896; min-width:38px; text-align:right; font-variant-numeric:tabular-nums; }
  .graph-hint { font-size:11px; color:#5c6773; padding:0 14px 10px; }
  h2 { border-bottom:1px solid #1f2933; padding-bottom:7px; font-size:17px; margin-top:34px; }
  .idea { border:1px solid #1f2933; border-left:4px solid #475569; border-radius:9px;
    padding:14px 18px; margin:12px 0; scroll-margin-top:14px; }
  .idea.done { border-left-color:#14532d; } .idea.doing { border-left-color:#1e3a8a; }
  .idea.blocked { border-left-color:#7c2d12; } .idea.endpoint { border-left-color:#581c87; }
  .idea h3 { margin:0 0 10px; font-size:16px; }
  .idea.flash { animation: flash 1.2s ease-out; }
  @keyframes flash { from { background:#1d4ed855; } to { background:transparent; } }
  .badge { font-size:11px; padding:2px 8px; border-radius:10px; background:#1f2933; color:#93a1b0;
    font-weight:normal; margin-left:8px; }
  .badge.end { background:#581c87; color:#faf5ff; }
  .iid { float:right; font-size:12px; color:#5c6773; font-weight:normal; }
  dl { margin:0; display:grid; grid-template-columns:max-content 1fr; gap:5px 18px; }
  dt { color:#7d8896; white-space:nowrap; } dd { margin:0; }
  code { background:#161b22; border-radius:4px; padding:1px 6px; font-size:13px; }
  .none { color:#4b5563; }
  .signoff { font-size:12px; color:#7d8896; }
  .edges { margin:11px 0 0; font-size:13px; }
  .edges b { color:#7d8896; font-weight:normal; margin-right:4px; }
  .xlink { display:inline-block; background:#161b22; border:1px solid #1f2933; border-radius:5px;
    padding:1px 8px; margin:2px 4px 2px 0; color:#7dd3fc; text-decoration:none; font-size:12px; }
  .xlink:hover { border-color:#7dd3fc; }
  .log { margin:10px 0 0; font-size:12px; color:#7d8896; }
  .log summary { cursor:pointer; } .log div { margin:4px 0 0 14px; }

  /* \u2500\u2500 editing \u2500\u2500 read view and write view swap; only one is ever displayed. */
  .rw { display:none; }
  .idea.editing .ro { display:none; }
  .idea.editing .rw { display:inline-block; }
  .idea.editing dd .rw { display:block; }
  textarea.rw, input.rw, select.rw { width:100%; font:inherit; font-size:14px; color:#e6edf3;
    background:#0d1117; border:1px solid #30363d; border-radius:6px; padding:6px 8px; resize:vertical; }
  input.rw { width:auto; min-width:min(24em,100%); font-size:16px; }
  select.rw { width:auto; font-size:12px; padding:2px 6px; }
  textarea.rw:focus, input.rw:focus, select.rw:focus { outline:none; border-color:#7dd3fc; }
  .edit-toggle { margin-left:8px; font:inherit; font-size:11px; cursor:pointer; padding:2px 9px;
    background:#161b22; color:#93a1b0; border:1px solid #232c36; border-radius:10px; }
  .edit-toggle:hover { border-color:#7dd3fc; color:#7dd3fc; }
  /* \u6539\u8FC7\u7684\u5730\u65B9\u8981\u770B\u5F97\u89C1 \u2014\u2014 \u63D0\u4EA4\u4E4B\u524D\uFF0C\u8FD9\u662F\u552F\u4E00\u7684\u300C\u54EA\u91CC\u52A8\u8FC7\u300D\u7684\u7EBF\u7D22\u3002 */
  .dirty > .rw, h3.dirty .rw { border-color:#eab308; background:#1c1917; }
  dd.dirty::after { content:"\u5DF2\u6539"; font-size:11px; color:#eab308; margin-left:6px; }
  .idea.dirty { border-left-color:#eab308; }
  #draft-banner, #restore { border:1px solid #3f3f18; background:#1c1917; color:#fde68a;
    border-radius:9px; padding:10px 14px; margin:0 0 14px; font-size:13px; }
  #restore { border-color:#4c1d95; background:#16121f; color:#ddd6fe; }
  #restore-list div { display:flex; gap:9px; align-items:center; margin:7px 0 0; }
  #restore-list span { flex:1; color:#a5a2b8; font-size:12px; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
  #restore button, #draft-banner button { font:inherit; font-size:11px; cursor:pointer; padding:2px 9px;
    background:#161b22; color:#c3ced9; border:1px solid #232c36; border-radius:10px; }
  #restore button:hover, #draft-banner button:hover { border-color:#7dd3fc; color:#7dd3fc; }

  /* \u2500\u2500 structure editing \u2500\u2500 edges, new ideas, pending deletions. */
  .chip { display:inline-flex; align-items:center; gap:2px; margin:2px 4px 2px 0; }
  .chip .xlink { margin:0; border-top-right-radius:0; border-bottom-right-radius:0; }
  .cut { font:inherit; font-size:11px; line-height:1; cursor:pointer; padding:2px 6px;
    background:#161b22; color:#7d8896; border:1px solid #1f2933; border-left:0;
    border-radius:0 5px 5px 0; }
  .cut:hover { color:#fca5a5; border-color:#7f1d1d; }
  select.rw-edge { font:inherit; font-size:11px; margin-left:6px; padding:2px 6px; color:#93a1b0;
    background:#0d1117; border:1px solid #232c36; border-radius:10px; cursor:pointer; }
  select.rw-edge:hover { border-color:#7dd3fc; color:#7dd3fc; }
  #new-idea { font:inherit; font-size:12px; cursor:pointer; padding:3px 11px; margin-left:10px;
    background:#161b22; color:#93a1b0; border:1px solid #232c36; border-radius:11px; vertical-align:2px; }
  #new-idea:hover { border-color:#7dd3fc; color:#7dd3fc; }
  .edit-toggle.danger:hover { border-color:#fca5a5; color:#fca5a5; }
  /* \u5F85\u5220\u662F\u6807\u8BB0\uFF0C\u4E0D\u662F\u6D88\u5931 \u2014\u2014 \u4EBA\u8981\u80FD\u770B\u89C1\u81EA\u5DF1\u5220\u4E86\u4EC0\u4E48\uFF0C\u5E76\u4E14\u6539\u4E3B\u610F\u3002 */
  .idea.removing { opacity:.55; border-left-color:#7f1d1d; }
  .idea.removing h3 > .ro, .idea.removing h3 > .rw { text-decoration:line-through; }
  .idea.incomplete { border-left-color:#a16207; }
  .idea.incomplete::before { content:"\u524D\u4E09\u95EE\u8FD8\u6CA1\u586B\u9F50\uFF0C\u63D0\u4EA4\u65F6\u4E0D\u4F1A\u5E26\u4E0A\u5B83"; display:block;
    font-size:11px; color:#eab308; margin:0 0 6px; }
  /* \u2500\u2500 worklists under the diagram \u2500\u2500 */
  .worklist { border:1px solid #1f2933; background:#0d1117; border-radius:9px;
    padding:9px 14px; margin:0 0 12px; font-size:13px; }
  .worklist > summary { cursor:pointer; color:#93a1b0; }
  .worklist > summary:hover { color:#7dd3fc; }
  .wl-row { display:flex; gap:10px; align-items:baseline; margin:7px 0 0; }
  .wl-row .xlink { margin:0; flex:none; }
  .wl-note { color:#7d8896; font-size:12px; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }

  #offline-note { border:1px solid #3f3f18; background:#1c1917; color:#fde68a;
    border-radius:9px; padding:10px 14px; margin:0 0 14px; font-size:13px; }

  /* \u2500\u2500 submitting \u2500\u2500 */
  #submit { font:inherit; font-size:11px; cursor:pointer; padding:2px 11px; margin-left:10px;
    background:#14532d; color:#f0fdf4; border:1px solid #166534; border-radius:10px; }
  #submit:hover:not(:disabled) { border-color:#86efac; }
  #submit:disabled { background:#161b22; color:#4b5563; border-color:#232c36; cursor:default; }
  #submit-panel { border:1px solid #1f3a2a; background:#0f1a14; color:#d7e6dc;
    border-radius:9px; padding:12px 16px; margin:0 0 14px; font-size:13px; }
  #submit-panel ul { margin:8px 0; padding-left:20px; }
  #submit-panel li { margin:2px 0; color:#a7c4b5; }
  #submit-panel button { font:inherit; font-size:12px; cursor:pointer; padding:3px 12px; margin-top:8px;
    background:#14532d; color:#f0fdf4; border:1px solid #166534; border-radius:10px; }
  #submit-panel button:hover { border-color:#86efac; }
  #submit-panel textarea { width:100%; margin-top:8px; font-family:ui-monospace,monospace; font-size:11px;
    color:#e6edf3; background:#0d1117; border:1px solid #30363d; border-radius:6px; padding:6px 8px; }
  #submit-panel code { font-size:12px; }

  /* \u2500\u2500 signing a manual check \u2500\u2500 */
  .sign-open { font:inherit; font-size:11px; cursor:pointer; padding:2px 9px; margin-left:8px;
    background:#161b22; color:#c084fc; border:1px solid #3b2a52; border-radius:10px; }
  .sign-open:hover { border-color:#c084fc; }
  #sign-panel { border:1px solid #3b2a52; background:#150f1c; color:#e2d9ee;
    border-radius:9px; padding:12px 16px; margin:0 0 14px; font-size:13px; }
  #sign-panel .what { color:#c9b8dd; margin:6px 0 10px; padding-left:10px; border-left:2px solid #3b2a52; }
  #sign-panel label { display:block; margin:8px 0 3px; font-size:12px; color:#a89bb8; }
  #sign-panel input, #sign-panel textarea { width:100%; font:inherit; font-size:13px; color:#e6edf3;
    background:#0d1117; border:1px solid #30363d; border-radius:6px; padding:6px 8px; }
  #sign-panel button { font:inherit; font-size:12px; cursor:pointer; padding:3px 12px; margin-top:10px;
    background:#4c1d95; color:#f5f3ff; border:1px solid #6d28d9; border-radius:10px; }
  #sign-panel button:hover { border-color:#c084fc; }
  #sign-panel .warn { color:#fca5a5; font-size:12px; margin-top:6px; }
</style></head><body>
<h1>${esc(g.project ?? "idea graph")} \u2014 \u60F3\u6CD5\u56FE</h1>
<p class="overview">${esc(g.overview)}</p>
<div id="restore" hidden>\u53D1\u73B0 <b><span id="restore-count">0</span></b> \u5904\u672A\u63D0\u4EA4\u7684\u6539\u52A8\uFF08\u4E0A\u6B21\u5173\u6389\u9875\u9762\u65F6\u6CA1\u6709\u63D0\u4EA4\uFF09\u3002
  \u9010\u6761\u786E\u8BA4\u8981\u4E0D\u8981\u6062\u590D \u2014\u2014 \u672C\u5730\u7F51\u9875\u7684\u5B58\u50A8\u4E0D\u6B62\u8FD9\u4E00\u9875\u80FD\u5199\uFF0C\u6240\u4EE5\u8FD9\u4E00\u6B65\u4E0D\u4F1A\u81EA\u52A8\u505A\uFF1A
  <div id="restore-list"></div></div>
<div id="draft-banner" hidden><b><span id="draft-count">0</span></b> \u5904\u672A\u63D0\u4EA4\u7684\u6539\u52A8<span id="draft-note"></span>
  <button id="submit" disabled>\u63D0\u4EA4</button></div>
<div id="submit-panel" hidden></div>
<div id="sign-panel" hidden></div>
<p class="legend">
  <i class="sw" style="background:#14532d"></i>\u5DF2\u5B8C\u6210
  <i class="sw" style="background:#1e3a8a"></i>\u8FDB\u884C\u4E2D
  <i class="sw" style="background:#334155"></i>\u5F85\u529E
  <i class="sw" style="background:#7c2d12"></i>\u53D7\u963B
  <i class="sw" style="background:#581c87"></i>\u7EC8\u70B9
  \xB7 ${esc(counts)} \xB7 \u70B9\u51FB\u4EFB\u610F\u8282\u70B9\u67E5\u770B\u8BE6\u60C5
</p>
<div class="graph">
  <div class="graph-tools">
    <button data-zoom="out" title="\u7F29\u5C0F">\u2212</button>
    <button data-zoom="in" title="\u653E\u5927">\uFF0B</button>
    <button data-zoom="fit" class="wide" title="\u9002\u5E94\u7A97\u53E3">\u9002\u5E94</button>
    <button data-zoom="reset" class="wide" title="\u56DE\u5230 100%">1:1</button>
    <span class="zoom-level">100%</span>
  </div>
  <div class="viewport"><div class="canvas"><pre class="mermaid">${mermaid}</pre></div></div>
</div>
<p class="graph-hint">\u6EDA\u8F6E\u7F29\u653E\uFF08\u4EE5\u5149\u6807\u4E3A\u4E2D\u5FC3\uFF09\xB7 \u62D6\u62FD\u5E73\u79FB \xB7 \u70B9\u51FB\u8282\u70B9\u770B\u8BE6\u60C5</p>
${worklist("\u5F85\u4EBA\u5DE5\u9A8C\u8BC1", g.ideas.filter(awaitingSignature).map((i) => ({
    id: i.id,
    name: i.name,
    note: i.verify?.manual ?? ""
  })))}
${worklist("\u8FDB\u884C\u4E2D", g.ideas.filter((i) => i.status === "doing").map((i) => {
    const waiting = waitingOn(i).map((n) => map.get(n).name || n);
    return { id: i.id, name: i.name, note: waiting.length ? `\u5728\u7B49 ${waiting.join("\u3001")}` : "\u6CA1\u6709\u524D\u7F6E\u6321\u7740\u5B83" };
  }))}
<h2>\u60F3\u6CD5\u8BE6\u60C5 <button id="new-idea">\uFF0B \u65B0\u5EFA\u60F3\u6CD5</button></h2>
<div id="offline-note" hidden>\u56FE\u6682\u65F6\u4E0D\u53EF\u7528\uFF08\u79BB\u7EBF\uFF0C\u753B\u56FE\u8981\u8054\u7F51\u53D6\u4E00\u4E2A\u7B2C\u4E09\u65B9\u5E93\uFF09\u2014\u2014 \u7F16\u8F91\u4E0E\u63D0\u4EA4\u7167\u5E38\u3002</div>
<div id="cards">
${g.ideas.map(card).join("\n")}
</div>
<!-- The same text the engine ran to draw the diagram above. A classic script,
     so it defines one global both module scripts below can reach. -->
<script id="mermaid-source-fn">${MERMAID_SOURCE_FN}</script>
<script id="ledger-fn">${LEDGER_FN}</script>
<script type="application/json" id="graph-data" data-fingerprint="${attr(source ? fingerprint(source) : "")}" data-draft-key="aidev-ideas-draft:${attr(projectDir ? fingerprint(projectDir) : "")}" data-project="${attr(projectDir)}" data-token="${attr(token)}">${// The page's one data model. `<` is escaped so no idea's text can close this
  // tag, and the type keeps the browser from executing it whatever it holds.
  JSON.stringify(g).replace(/</g, "\\u003c")}</script>
<script type="module">
  // \u2500\u2500 editing \u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500
  // Its own module: if the mermaid CDN above is unreachable, that import throws
  // and takes its whole module with it \u2014 editing must not go down with it.
  const dataEl = document.getElementById("graph-data");
  const DATA = JSON.parse(dataEl.textContent);
  const DRAFT_KEY = dataEl.dataset.draftKey;
  const FINGERPRINT = dataEl.dataset.fingerprint;
  const PROJECT = dataEl.dataset.project || "";

  // One book for everything a person changes. It measures against the graph as
  // this page was written, so a field typed back to its old value stops being a
  // change; and its envelope is both the draft below and the file that will
  // reach disk \u2014 one format, not three.
  const ledger = createLedger(DATA, FINGERPRINT, PROJECT);
  let storageOk = true;

  const banner = document.getElementById("draft-banner");
  const count = document.getElementById("draft-count");
  const note = document.getElementById("draft-note");
  const inputFor = (id, f) => document.querySelector('[data-idea="' + id + '"][data-field="' + f + '"]');

  const touched = (id) => ledger.ops().some((o) =>
    o.id === id || o.tmp === id || o.from === id || o.to === id);

  const submitBtn = document.getElementById("submit");
  const panel = document.getElementById("submit-panel");

  function refresh() {
    count.textContent = String(ledger.ops().length);
    banner.hidden = ledger.isEmpty();
    note.textContent = storageOk ? "" : "\uFF08\u6D4F\u89C8\u5668\u672C\u5730\u5B58\u50A8\u7528\u4E0D\u4E86\uFF0C\u8349\u7A3F\u53EA\u5728\u5185\u5B58\u91CC \u2014\u2014 \u5173\u9875\u5373\u4E22\uFF09";
    if (submitBtn) submitBtn.disabled = ledger.isEmpty();
  }

  /** The single way a typed edit enters the book \u2014 typing or restoring. */
  function applyChange(id, f, value) {
    const el = inputFor(id, f);
    if (el) el.value = value;                    // .value / .textContent only \u2014 never as markup
    if (f === "status") ledger.setStatus(id, value); else ledger.setField(id, f, value);
    const dirty = ledger.ops().some((o) =>
      o.id === id && (o.field === f || (f === "status" && o.op === "status")));
    const box = el && (el.closest("[data-f]") || el.parentElement);
    if (box) box.classList.toggle("dirty", dirty);
    const card = document.getElementById(id);
    if (card) card.classList.toggle("dirty", touched(id));
    if (typeof markIncomplete === "function") markIncomplete(id);
    refresh();
    writeDraft();
  }

  // A draft is a safety net, never the transport: submitting reads the book.
  function writeDraft() {
    try {
      localStorage.setItem(DRAFT_KEY, JSON.stringify(ledger.envelope()));
    } catch (e) { storageOk = false; refresh(); }
  }
  function clearDraft() {
    try { localStorage.removeItem(DRAFT_KEY); } catch (e) { storageOk = false; }
  }

  const onEdit = (e) => {
    const el = e.target;
    if (!el || !el.dataset || !el.dataset.field) return;
    applyChange(el.dataset.idea, el.dataset.field, el.value);
  };
  document.addEventListener("input", onEdit);
  document.addEventListener("change", onEdit);

  document.querySelectorAll("[data-edit]").forEach((b) => b.addEventListener("click", () => {
    const card = document.getElementById(b.getAttribute("data-edit"));
    b.textContent = card.classList.toggle("editing") ? "\u5B8C\u6210" : "\u7F16\u8F91";
  }));

  /** One line a person can judge without opening anything. */
  function describeOp(o) {
    const trim = (s) => String(s === undefined || s === null ? "" : s).replace(/\\s+/g, " ").slice(0, 60);
    if (o.op === "set") return o.id + " \xB7 " + o.field + " \u2192 " + trim(o.new);
    if (o.op === "status") return o.id + " \xB7 \u72B6\u6001 " + o.from + " \u2192 " + o.to;
    if (o.op === "add") return "\u65B0\u5EFA\u60F3\u6CD5 \xB7 " + trim(o.fields && o.fields.name);
    if (o.op === "remove") return "\u5220\u9664\u60F3\u6CD5 " + o.id;
    if (o.op === "link") return "\u8FDE\u4E0A\u524D\u7F6E " + o.from + " \u2192 " + o.to;
    if (o.op === "unlink") return "\u65AD\u5F00\u524D\u7F6E " + o.from + " \u2192 " + o.to;
    return o.op;
  }

  /** Put one restored operation back into the book, DOM included where there is one. */
  function restoreOp(o) {
    if (o.op === "set") { applyChange(o.id, o.field, o.new); return; }
    if (o.op === "status") { applyChange(o.id, "status", o.to); return; }
    if (o.op === "add") ledger.addIdea(o.fields);
    if (o.op === "remove") ledger.removeIdea(o.id);
    if (o.op === "link") ledger.link(o.from, o.to);
    if (o.op === "unlink") ledger.unlink(o.from, o.to);
    refresh();
    writeDraft();
  }

  // Defined after restoreOp on purpose: nothing between reading storage and the
  // human's click may put a draft into the book, and that ordering is what the
  // test asserts by slicing the script between the two.
  function readDraft() {
    try {
      const raw = localStorage.getItem(DRAFT_KEY);
      if (!raw) return { ops: [], stale: false };
      const env = JSON.parse(raw);
      // The stored digest is compared, not ignored: a draft written against an
      // older graph must say so before anyone puts it back.
      return { ops: (env && env.ops) || [], stale: !env || env.baseDigest !== FINGERPRINT };
    } catch (e) { storageOk = false; return { ops: [], stale: false }; }
  }

  // Restoring is never silent. Under file:// every local page shares one
  // storage area, so what is in there is not proof a person put it there.
  const draft = readDraft();
  if (draft.ops.length) {
    const panel = document.getElementById("restore");
    const list = document.getElementById("restore-list");
    document.getElementById("restore-count").textContent = String(draft.ops.length);
    if (draft.stale) {
      // Into the panel, NOT into the list. The done() helper below hides the
      // panel once the list is empty, so anything parked in the list that is
      // not a row keeps the count above zero forever \u2014 the panel never closes
      // and the draft is never cleared, so it returns on every reload.
      // (No backticks in here: this whole block lives inside a template
      // literal, and one would close it.)
      const warn = document.createElement("div");
      warn.className = "restore-warn";
      warn.textContent = "\u6CE8\u610F\uFF1A\u8FD9\u4EFD\u8349\u7A3F\u662F\u5BF9\u7740\u53E6\u4E00\u4E2A\u7248\u672C\u7684\u56FE\u5199\u7684\uFF0C\u6062\u590D\u4E4B\u524D\u8BF7\u9010\u6761\u786E\u8BA4\u5B83\u662F\u5426\u8FD8\u8BF4\u5F97\u901A\u3002";
      panel.insertBefore(warn, list);
    }
    panel.hidden = false;
    const done = () => {
      if (list.children.length) return;
      panel.hidden = true;
      // Persist whatever ended up in the book, rather than wiping it: restoring
      // a row writes it to the draft, and clearing unconditionally right after
      // would lose exactly what was just restored on the next reload.
      if (ledger.isEmpty()) clearDraft(); else writeDraft();
    };
    for (const o of draft.ops) {
      const row = document.createElement("div");
      const label = document.createElement("span");
      label.textContent = describeOp(o);
      const yes = document.createElement("button");
      yes.textContent = "\u6062\u590D";
      yes.setAttribute("data-restore", o.op);
      yes.addEventListener("click", () => { restoreOp(o); row.remove(); done(); });
      const no = document.createElement("button");
      no.textContent = "\u4E22\u5F03";
      no.addEventListener("click", () => { row.remove(); done(); });
      row.append(label, yes, no);
      list.append(row);
    }
  }

  // \u2500\u2500 structure \u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500
  const REQUIRED = ["name", "what", "why", "expected"];
  const addOp = (id) => ledger.ops().find((o) => o.op === "add" && o.tmp === id);

  /** A field's value with everything in the book applied. */
  function effectiveField(id, f) {
    const set = ledger.ops().filter((o) => o.op === "set" && o.id === id && o.field === f).pop();
    if (set) return set.new;
    const add = addOp(id);
    if (add) return (add.fields && add.fields[f]) || "";
    const idea = DATA.ideas.find((i) => i.id === id);
    return (idea && idea[f]) || "";
  }
  const nameOf = (id) => effectiveField(id, "name") || id;

  /** The graph as it would look with the whole book applied. */
  function snapshot() {
    const gone = new Set(ledger.ops().filter((o) => o.op === "remove").map((o) => o.id));
    const out = [];
    for (const idea of DATA.ideas) {
      if (gone.has(idea.id)) continue;
      const patched = Object.assign({}, idea);
      for (const o of ledger.ops()) {
        if (o.op === "set" && o.id === idea.id) patched[o.field] = o.new;
        if (o.op === "status" && o.id === idea.id) patched.status = o.to;
      }
      patched.needs = ledger.needsOf(idea.id);
      out.push(patched);
    }
    for (const o of ledger.ops()) {
      if (o.op !== "add") continue;
      out.push({ id: o.tmp, name: nameOf(o.tmp), status: "todo", needs: ledger.needsOf(o.tmp) });
    }
    return { version: DATA.version, endpoints: DATA.endpoints, ideas: out };
  }

  // The diagram module may never arrive (it loads from a CDN). Empty its source
  // out of the page right now: an undrawn block shows the raw flowchart text as
  // body copy, which is worse than showing nothing at all.
  const pre = document.querySelector("pre.mermaid");
  if (pre) pre.textContent = "";
  const offline = document.getElementById("offline-note");

  window.currentMermaidSource = () => buildMermaidSource(snapshot());
  window.graphReady = () => { if (offline) offline.hidden = true; };

  function redraw() {
    if (typeof window.redrawGraph === "function") { window.redrawGraph(window.currentMermaidSource()); return; }
    if (offline) offline.hidden = false;
  }

  function markIncomplete(id) {
    const c = document.getElementById(id);
    if (!c || !addOp(id)) return;
    c.classList.toggle("incomplete", REQUIRED.some((f) => !String(effectiveField(id, f)).trim()));
  }

  /** Rebuild one card's prerequisite chips from the book. */
  function renderNeeds(id) {
    const box = document.querySelector('[data-needs-of="' + id + '"] .chips');
    if (!box) return;
    const needs = ledger.needsOf(id);
    box.replaceChildren();
    if (!needs.length) {
      const dash = document.createElement("span");
      dash.className = "none"; dash.textContent = "\u2014";
      box.append(dash);
      return;
    }
    for (const n of needs) {
      const chip = document.createElement("span");
      chip.className = "chip";
      const a = document.createElement("a");
      a.className = "xlink"; a.href = "#" + n;
      a.setAttribute("data-goto", n);
      a.textContent = nameOf(n);
      const cut = document.createElement("button");
      cut.className = "cut"; cut.textContent = "\xD7";
      cut.title = "\u65AD\u5F00\u8FD9\u6761\u524D\u7F6E";
      cut.setAttribute("data-unlink-from", n);
      cut.setAttribute("data-unlink-to", id);
      chip.append(a, cut);
      box.append(chip);
    }
  }

  function afterStructure(id) {
    if (id) renderNeeds(id);
    refresh();
    writeDraft();
    redraw();
  }

  // Delegated, so rebuilt chips and freshly created cards need no re-wiring.
  document.addEventListener("click", (e) => {
    const t = e.target;
    if (!t || !t.getAttribute) return;
    const from = t.getAttribute("data-unlink-from");
    if (from) {
      const to = t.getAttribute("data-unlink-to");
      ledger.unlink(from, to);
      afterStructure(to);
      return;
    }
    const rm = t.getAttribute("data-remove");
    if (rm) {
      const c = document.getElementById(rm);
      const pending = ledger.ops().some((o) => o.op === "remove" && o.id === rm);
      // Marked, not gone \u2014 and a second click takes it back.
      if (pending) {
        ledger.load({ v: 1, project: PROJECT, baseDigest: FINGERPRINT,
          ops: ledger.ops().filter((o) => !(o.op === "remove" && o.id === rm)) });
      } else {
        ledger.removeIdea(rm);
      }
      if (c) c.classList.toggle("removing", !pending);
      afterStructure(null);
    }
  });

  document.addEventListener("change", (e) => {
    const t = e.target;
    const to = t && t.getAttribute && t.getAttribute("data-link-to");
    if (!to || !t.value) return;
    ledger.link(t.value, to);
    t.value = "";
    afterStructure(to);
  });

  // \u2500\u2500 signing a manual check \u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500
  // The one conclusion a machine may not reach. What makes this a signature
  // rather than a ritual is that the person types a sentence: a checkbox would
  // record only that somebody clicked, where a sentence records what they
  // judged. And the panel shows the check being signed, because signing a
  // blank page and signing a specific claim are not the same act.
  const signPanel = document.getElementById("sign-panel");
  const WHO_KEY = DRAFT_KEY + ":who";

  document.addEventListener("click", (e) => {
    const t = e.target;
    const id = t && t.getAttribute && t.getAttribute("data-sign");
    if (!id) return;

    let remembered = "";
    try { remembered = localStorage.getItem(WHO_KEY) || ""; } catch (err) { /* fine without it */ }

    signPanel.hidden = false;
    signPanel.replaceChildren();
    const head = document.createElement("div");
    // D27: the page asks; the signature itself lands when the person answers the
    // one-time challenge in the agent's chat. Say so, or the button lies.
    head.textContent = "\u7ED9 " + id + " \u7684\u4EBA\u5DE5\u9A8C\u8BC1\u63D0\u7B7E\u5B57\u8BF7\u6C42\uFF08\u63D0\u4EA4\u540E\u56DE\u4E00\u53E5\u4E00\u6B21\u6027\u53E3\u4EE4\u624D\u771F\u7684\u7B7E\u4E0A\uFF09\u3002\u4F60\u8981\u7B7E\u7684\u662F\u8FD9\u4EF6\u4E8B\uFF1A";
    const what = document.createElement("div");
    what.className = "what";
    what.textContent = t.getAttribute("data-manual") || "";
    const whoLabel = document.createElement("label");
    whoLabel.textContent = "\u4F60\u7684\u540D\u5B57";
    const who = document.createElement("input");
    who.id = "sign-who";
    who.value = remembered;
    const wordsLabel = document.createElement("label");
    wordsLabel.textContent = "\u4F60\u81EA\u5DF1\u7684\u8BDD \u2014\u2014 \u4F60\u770B\u5230\u4E86\u4EC0\u4E48\u3001\u51ED\u4EC0\u4E48\u8BF4\u5B83\u8FC7\u4E86";
    const words = document.createElement("textarea");
    words.id = "sign-words";
    words.rows = 3;
    const go = document.createElement("button");
    go.id = "sign-go";
    go.textContent = "\u7B7E\u5B57";
    const warn = document.createElement("div");
    warn.className = "warn";

    go.addEventListener("click", () => {
      // Refused here as well as in the engine: a blank signature should not get
      // as far as the change file.
      if (!ledger.sign(id, who.value, words.value)) {
        warn.textContent = "\u540D\u5B57\u548C\u8BDD\u90FD\u5F97\u586B \u2014\u2014 \u7A7A\u767D\u7684\u7B7E\u540D\u7B49\u4E8E\u6CA1\u7B7E\u3002";
        return;
      }
      try { localStorage.setItem(WHO_KEY, String(who.value).trim()); } catch (err) { /* fine */ }
      signPanel.hidden = true;
      refresh();
      writeDraft();
    });

    signPanel.append(head, what, whoLabel, who, wordsLabel, words, go, warn);
    signPanel.scrollIntoView({ block: "center" });
  });

  // \u2500\u2500 submitting \u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500
  // Two roads out, carrying the same envelope: a request when a local service
  // is up, a file when there isn't one. The probe must fail quietly \u2014 the copy
  // somebody opened by double-clicking takes that road every time.
  const TOKEN = dataEl.dataset.token || "";

  function say(text) {
    panel.hidden = false;
    panel.replaceChildren();
    const line = document.createElement("div");
    line.textContent = text;
    panel.append(line);
    return panel;
  }

  async function haveServer() {
    if (!TOKEN) return false;                  // no token means nobody served this page
    try {
      const r = await fetch("/health");
      return (await r.json()).ok === true;
    } catch (e) { return false; }
  }

  async function post(envelope, confirm) {
    const r = await fetch("/changes", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ token: TOKEN, confirm: confirm, envelope: envelope }),
    });
    return await r.json();
  }

  /** No service: hand the envelope over as a file, and say what to do with it. */
  function handOverFile(envelope) {
    const text = JSON.stringify(envelope, null, 2);
    const box = say("\u8FD9\u53F0\u673A\u5668\u4E0A\u6CA1\u6709\u5F00\u7740\u672C\u5730\u670D\u52A1\uFF0C\u6240\u4EE5\u6539\u52A8\u5B58\u6210\u4E86\u4E00\u4E2A\u6587\u4EF6\u3002");
    const how = document.createElement("div");
    // D14 + D34: the engine a project actually has is the single-file bundle at
    // the host-neutral plugin root, so that is the one command this page names \u2014
    // interpolated from ENGINE_CMD, not retyped, because a hand-copied path that
    // merely happens to match today is exactly what D14 forbids.
    how.textContent = "\u628A\u5B83\u653E\u8FDB " + (PROJECT || "<\u9879\u76EE\u76EE\u5F55>") + "/ideas/ \uFF0C\u7136\u540E\u8DD1\uFF1A"
      + " ${ENGINE_CMD} apply";
    // The last tier is a textarea on purpose: a download can be blocked and the
    // clipboard is often unavailable under file://, but selecting text in a box
    // cannot fail. The only requirement of a fallback is that it never fails.
    const area = document.createElement("textarea");
    area.id = "submit-text";
    area.rows = 8;
    area.value = text;
    box.append(how, area);
    try {
      const url = URL.createObjectURL(new Blob([text], { type: "application/json" }));
      const a = document.createElement("a");
      a.href = url;
      a.download = "changes.json";
      document.body.append(a);                 // must be in the document before the click
      a.click();
      a.remove();
      setTimeout(() => { try { URL.revokeObjectURL(url); } catch (e) {} }, 1000);
    } catch (e) { /* blocked download is fine \u2014 the box above is always there */ }
  }

  async function submitChanges() {
    if (ledger.isEmpty()) return;
    const envelope = ledger.envelope();
    if (!(await haveServer())) { handOverFile(envelope); return; }

    const preview = await post(envelope, false);
    if (!preview.ok) { say("\u5199\u56DE\u88AB\u62D2\uFF1A" + preview.reason); return; }

    const box = say("\u8FD9\u4E00\u6B21\u4F1A\u5199\u8FDB\u56FE\u91CC\u7684\u6539\u52A8\uFF1A");
    const list = document.createElement("ul");
    for (const line of preview.changed || []) {
      const li = document.createElement("li");
      li.textContent = line;
      list.append(li);
    }
    const go = document.createElement("button");
    go.id = "confirm-write";
    go.textContent = "\u786E\u8BA4\uFF0C\u5199\u8FDB\u56FE\u91CC";
    go.addEventListener("click", async () => {
      const done = await post(envelope, true);
      if (!done.ok) { say("\u5199\u56DE\u88AB\u62D2\uFF1A" + done.reason); return; }
      // The draft is deliberately left alone. A write-back can still be refused
      // later, and the person's edits must not be the thing that gets destroyed.
      say("\u5DF2\u63D0\u4EA4\uFF0C\u5199\u56DE\u4E86 " + (done.changed || []).length + " \u5904\u6539\u52A8\u3002\u5237\u65B0\u9875\u9762\u5C31\u80FD\u770B\u5230\u65B0\u56FE\u3002");
      const again = document.createElement("button");
      again.textContent = "\u5237\u65B0\u9875\u9762";
      again.addEventListener("click", () => { try { location.reload(); } catch (e) {} });
      panel.append(again);
    });
    box.append(list, go);
  }

  if (submitBtn) submitBtn.addEventListener("click", () => { submitChanges(); });

  document.getElementById("new-idea").addEventListener("click", () => {
    const tmp = ledger.addIdea({ name: "", what: "", why: "", expected: "" });
    const proto = document.querySelector(".idea");
    const el = proto.cloneNode(true);
    el.id = tmp;
    el.className = "idea todo editing";
    for (const attrName of ["data-idea", "data-edit", "data-remove", "data-needs-of", "data-link-to"]) {
      for (const n of el.querySelectorAll("[" + attrName + "]")) n.setAttribute(attrName, tmp);
    }
    for (const n of el.querySelectorAll("textarea, input")) n.value = "";
    for (const n of el.querySelectorAll(".ro")) n.textContent = "";
    for (const n of el.querySelectorAll(".iid")) n.textContent = tmp;
    for (const n of el.querySelectorAll("select.rw")) n.value = "todo";
    for (const n of el.querySelectorAll(".chips")) n.replaceChildren();
    // \u300C\u4EE3\u7801\u5728\u54EA\u300D\u300C\u5982\u4F55\u9A8C\u8BC1\u300D\u8FD9\u7C7B\u53EA\u8BFB\u683C\u6CA1\u6709 .ro\uFF0C\u4E0A\u9762\u51E0\u884C\u591F\u4E0D\u7740\u5B83\u4EEC\uFF0C
    // \u65B0\u60F3\u6CD5\u4F1A\u5E26\u7740\u4E0A\u4E00\u5F20\u5361\u7247\u7684\u4EE3\u7801\u8DEF\u5F84\u548C\u9A8C\u8BC1\u547D\u4EE4\u51FA\u751F \u2014\u2014 \u800C\u90A3\u6B63\u662F /ccbuild
    // \u7167\u7740\u53BB\u5199\u6587\u4EF6\u7684\u4E24\u6837\u4E1C\u897F\u3002\u6309\u300C\u4E0D\u662F\u53EF\u7F16\u8F91\u5B57\u6BB5\u7684\u683C\u4E00\u5F8B\u6E05\u7A7A\u300D\u6765\u6E05\uFF0C
    // \u4EE5\u540E\u518D\u52A0\u53EA\u8BFB\u683C\u4E5F\u4E0D\u4F1A\u6F0F\u3002
    for (const n of el.querySelectorAll("dd:not([data-f])")) n.textContent = "\u2014";
    for (const n of el.querySelectorAll(".log, .badge.end")) n.remove();
    document.getElementById("cards").append(el);
    markIncomplete(tmp);
    afterStructure(tmp);
  });

  refresh();
</script>
<script type="module">
  // \u2500\u2500 the diagram \u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500
  // Last on purpose: this module fetches from a CDN and awaits at the top level,
  // so anything after it would not initialise until the network answered.
  // Editing above must never wait on that.
  import mermaid from "https://cdn.jsdelivr.net/npm/mermaid@11/dist/mermaid.esm.min.mjs";
  // ELK packs big graphs far tighter than the default dagre and routes edges
  // with fewer crossings; if its CDN module fails to load, dagre still renders.
  let layout = "dagre";
  try {
    const elk = await import("https://cdn.jsdelivr.net/npm/@mermaid-js/layout-elk@0/dist/mermaid-layout-elk.esm.min.mjs");
    mermaid.registerLayoutLoaders(elk.default);
    layout = "elk";
  } catch (e) { console.warn("ELK layout unavailable, falling back to dagre", e); }
  mermaid.initialize({
    startOnLoad: false, securityLevel: "loose", theme: "dark", layout,
    elk: { mergeEdges: true, nodePlacementStrategy: "LINEAR_SEGMENTS" },
    flowchart: { nodeSpacing: 30, rankSpacing: 55, curve: "basis", padding: 8 },
  });

  // \u2500\u2500 pan / zoom \u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500
  // Plain CSS transform on a wrapper: no library, no dependency, and the SVG
  // stays a real SVG so mermaid's node clicks keep working.
  const viewport = document.querySelector(".viewport");
  const canvas = document.querySelector(".canvas");
  const readout = document.querySelector(".zoom-level");
  const MIN = 0.15, MAX = 6;
  let k = 1, tx = 0, ty = 0;

  const clamp = (v) => Math.min(MAX, Math.max(MIN, v));

  /** The diagram's own coordinate size, from the viewBox \u2014 CSS cannot skew it. */
  function natural() {
    const svg = canvas.querySelector("svg");
    const box = svg && svg.viewBox && svg.viewBox.baseVal;
    return box && box.width ? { svg, w: box.width, h: box.height } : null;
  }

  function apply() {
    // Zoom by resizing the SVG so the vectors are re-rendered at that size, and
    // pan by translating the wrapper. Translation alone never blurs.
    const n = natural();
    if (n) { n.svg.style.width = n.w * k + "px"; n.svg.style.height = n.h * k + "px"; }
    canvas.style.transform = \`translate(\${tx}px, \${ty}px)\`;
    readout.textContent = Math.round(k * 100) + "%";
  }
  /** Zoom about a point in viewport coordinates, so the cursor stays put. */
  function zoomAt(px, py, factor) {
    const next = clamp(k * factor);
    if (next === k) return;
    tx = px - (px - tx) * (next / k);
    ty = py - (py - ty) * (next / k);
    k = next;
    apply();
  }
  function fit() {
    const n = natural();
    if (!n) return;
    const view = viewport.getBoundingClientRect();
    // Shrink to fit, never enlarge: blowing a small diagram up to 235% is not
    // what "fit" means to anyone looking at a flowchart. Centre it either way.
    k = clamp(Math.min(1, (view.width - 28) / n.w, (view.height - 28) / n.h));
    tx = (view.width - n.w * k) / 2;
    ty = (view.height - n.h * k) / 2;
    apply();
  }

  viewport.addEventListener("wheel", (e) => {
    e.preventDefault();
    const r = viewport.getBoundingClientRect();
    zoomAt(e.clientX - r.left, e.clientY - r.top, Math.exp(-e.deltaY * 0.0015));
  }, { passive: false });

  let dragging = false, lastX = 0, lastY = 0, moved = 0;
  viewport.addEventListener("pointerdown", (e) => {
    dragging = true; moved = 0; lastX = e.clientX; lastY = e.clientY;
    viewport.classList.add("dragging");
  });
  viewport.addEventListener("pointermove", (e) => {
    if (!dragging) return;
    const dx = e.clientX - lastX, dy = e.clientY - lastY;
    moved += Math.abs(dx) + Math.abs(dy);
    // Capture only once it is really a drag. Capturing on pointerdown retargets
    // the whole compatibility mouse sequence \u2014 including click \u2014 to .viewport,
    // so mermaid's per-node handler never fires and nodes stop opening cards.
    if (moved > 4 && !viewport.hasPointerCapture(e.pointerId)) viewport.setPointerCapture(e.pointerId);
    tx += dx; ty += dy; lastX = e.clientX; lastY = e.clientY;
    apply();
  });
  const endDrag = (e) => {
    if (!dragging) return;
    dragging = false;
    viewport.classList.remove("dragging");
    // A drag must not also register as a node click; a tap (< 4px) still should.
    if (moved > 4) { e.preventDefault(); e.stopPropagation(); }
  };
  viewport.addEventListener("pointerup", endDrag);
  viewport.addEventListener("pointercancel", endDrag);

  document.querySelectorAll(".graph-tools button").forEach((b) => {
    b.addEventListener("click", () => {
      const r = viewport.getBoundingClientRect();
      const action = b.getAttribute("data-zoom");
      if (action === "in") zoomAt(r.width / 2, r.height / 2, 1.25);
      else if (action === "out") zoomAt(r.width / 2, r.height / 2, 0.8);
      else if (action === "fit") fit();
      else { k = 1; tx = 0; ty = 0; apply(); }
    });
  });

  function gotoNode(id) {
    const el = document.getElementById(id);
    if (!el) return;
    el.scrollIntoView({ behavior: "smooth", block: "start" });
    el.classList.remove("flash"); void el.offsetWidth; el.classList.add("flash");
  }
  window.nodeClick = gotoNode;
  document.querySelectorAll("[data-goto]").forEach((a) =>
    a.addEventListener("click", (e) => { e.preventDefault(); gotoNode(a.getAttribute("data-goto")); }));

  /** Draw one source. mermaid stamps what it has processed, so replace the node. */
  async function draw(src) {
    const host = document.querySelector(".canvas");
    const pre = document.createElement("pre");
    pre.className = "mermaid";
    pre.textContent = src;                       // never markup
    host.replaceChildren(pre);
    await mermaid.run({ nodes: [pre] });
    for (let i = 0; i < 120 && !natural(); i++) await new Promise(requestAnimationFrame);
  }

  await draw(window.currentMermaidSource());
  fit();
  // The one bridge to the editing module above \u2014 the same trick as nodeClick.
  // It keeps the human's zoom and pan, because a redraw on every edit that
  // snapped back to the whole graph would make editing unusable.
  window.redrawGraph = (src) => { draw(src).then(() => apply()); };
  window.graphReady();
  new ResizeObserver(() => { if (k === 1 && tx === 0 && ty === 0) fit(); }).observe(viewport);
</script></body></html>`;
}
var DEFAULT_PORT = 8787;
var HOST = "127.0.0.1";
function readJson(req) {
  return new Promise((done, fail) => {
    let raw = "";
    req.on("data", (c) => {
      raw += c;
      if (raw.length > 4e6) fail(new Error("\u6539\u52A8\u6587\u4EF6\u592A\u5927\u4E86"));
    });
    req.on("end", () => {
      try {
        done(JSON.parse(raw || "null"));
      } catch (e) {
        fail(e);
      }
    });
    req.on("error", fail);
  });
}
function listenFrom(server, from, tries = 20) {
  return new Promise((done, fail) => {
    const attempt = (port, left) => {
      const onError = (error) => {
        server.removeListener("listening", onListening);
        if (error.code === "EADDRINUSE" && left > 0) {
          attempt(port + 1, left - 1);
          return;
        }
        fail(error);
      };
      const onListening = () => {
        server.removeListener("error", onError);
        done(port);
      };
      server.once("error", onError);
      server.once("listening", onListening);
      server.listen(port, HOST);
    };
    attempt(from, tries);
  });
}
async function serve(projectDir, file, opts = {}) {
  const token = randomBytes(16).toString("hex");
  const send = (res, code, body) => {
    const text = JSON.stringify(body);
    res.writeHead(code, { "content-type": "application/json; charset=utf-8" });
    res.end(text);
  };
  const server = createServer(async (req, res) => {
    try {
      const path = (req.url ?? "/").split("?")[0];
      if (req.method === "GET" && path === "/health") {
        send(res, 200, { ok: true });
        return;
      }
      if (req.method === "GET" && path === "/") {
        const text = readFileSync(file, "utf8");
        const graph = (0, import_yaml.parseDocument)(text).toJSON();
        res.writeHead(200, { "content-type": "text/html; charset=utf-8" });
        res.end(render(graph, text, projectDir, token));
        return;
      }
      if (req.method === "POST" && path === "/changes") {
        const body = await readJson(req);
        if (!body || body.token !== token) {
          send(res, 403, { ok: false, reason: "\u4EE4\u724C\u4E0D\u5BF9 \u2014\u2014 \u8FD9\u4E2A\u670D\u52A1\u53EA\u63A5\u53D7\u5B83\u81EA\u5DF1\u53D1\u51FA\u53BB\u7684\u90A3\u4E2A\u9875\u9762" });
          return;
        }
        const source = readFileSync(file, "utf8");
        const today = (/* @__PURE__ */ new Date()).toISOString().slice(0, 10);
        const result = applyChanges(source, body.envelope, today, projectDir);
        if (!result.ok) {
          send(res, 200, { ok: false, reason: result.reason });
          return;
        }
        if (body.confirm !== true) {
          send(res, 200, { ok: true, preview: true, changed: result.changed });
          return;
        }
        atomicWrite(file, result.text);
        appendServeLog(projectDir, result.changed ?? []);
        const graph = (0, import_yaml.parseDocument)(result.text).toJSON();
        const signs = requestSignatures(projectDir, graph, result.signRequests ?? [], today);
        for (const line of signs) console.log(line);
        send(res, 200, { ok: true, changed: result.changed, graph, signs });
        return;
      }
      send(res, 404, { ok: false, reason: "\u6CA1\u6709\u8FD9\u4E2A\u5730\u5740" });
    } catch (error) {
      send(res, 400, { ok: false, reason: String(error instanceof Error ? error.message : error) });
    }
  });
  const port = await listenFrom(server, opts.port ?? DEFAULT_PORT);
  const url = `http://${HOST}:${port}`;
  if (opts.open) openBrowser(url);
  return {
    host: HOST,
    port,
    token,
    url,
    close: () => new Promise((done) => server.close(() => done()))
  };
}
function appendServeLog(projectDir, changed) {
  if (changed.length === 0) return;
  try {
    const stamp = (/* @__PURE__ */ new Date()).toISOString().replace("T", " ").slice(0, 16);
    appendFileSync(
      logFile(projectDir),
      `- ${stamp}  \u7F51\u9875\u5199\u56DE ${changed.length} \u5904\uFF1A${changed.join("\uFF1B")}
`
    );
  } catch {
  }
}
function browserCommand(url) {
  if (platform === "win32") return ["cmd", ["/c", "start", "", url]];
  if (platform === "darwin") return ["open", [url]];
  return ["xdg-open", [url]];
}
function openBrowser(url) {
  try {
    if (env.AIDEV_NO_BROWSER) return;
    const [cmd, args2] = browserCommand(url);
    spawn(cmd, args2, { detached: true, stdio: "ignore" }).unref();
  } catch {
  }
}
var SEED = `version: 1
project: PROJECT_NAME
overview: >
  \u4E00\u6BB5\u8BDD\u8BF4\u6E05\u8FD9\u4E2A\u9879\u76EE\u5728\u505A\u4EC0\u4E48\u3002

# \u7EC8\u70B9\uFF1A\u4EC0\u4E48\u53EB"\u8FD9\u4E2A\u9879\u76EE\u505A\u5B8C\u4E86"\u3002\u6BCF\u4E2A\u90FD\u662F\u4E0B\u9762\u67D0\u4E2A\u60F3\u6CD5\u7684 id\u3002
endpoints: []

ideas: []
`;
var KEEP_RUNNING = -1;
var SUBCOMMANDS = [
  ["paths", ""],
  ["init", ""],
  ["migrate", "[--pick claude|cursor|codex] [--dry-run]"],
  ["scan", "[--reset] [--n 40] [--skipped]"],
  ["new", "<\u540D\u79F0> [--needs I-001,I-002]"],
  ["check", ""],
  ["status", ""],
  ["next", ""],
  ["show", "<id>"],
  ["log", "[id] [--n 10]"],
  ["set", "<id> <status>"],
  ["allow", "<path>"],
  ["render", ""],
  ["apply", "[file]"],
  ["serve", "[--port 4173] [--no-open]"],
  ["request-approval", "--gate decomposition|plan|red-waiver|manual-check [--node I-002[,I-003]] [--by \u4EBA\u540D]"],
  ["run-check", "<id> --phase red|green [--timeout \u79D2]"]
];
var usageOf = (name) => {
  const found = SUBCOMMANDS.find(([n]) => n === name);
  return `usage: ${ENGINE_CMD} ${name}${found?.[1] ? ` ${found[1]}` : ""}`;
};
var usageLines = () => [
  `usage: ${ENGINE_CMD} <\u5B50\u547D\u4EE4>`,
  ...SUBCOMMANDS.map(([name, args2]) => `  ${name}${args2 ? ` ${args2}` : ""}`),
  "  \u5171\u7528\u53C2\u6570\uFF1A[--file ideas/graph.yaml] [--project .] [--by who] [--note text] [--date YYYY-MM-DD]"
];
function flag(args2, name) {
  const i = args2.indexOf(`--${name}`);
  return i >= 0 ? args2[i + 1] : void 0;
}
function redraw(file, projectDir) {
  const out = file.replace(/\.ya?ml$/, ".html");
  const text = readFileSync(file, "utf8");
  const graph = (0, import_yaml.parseDocument)(text).toJSON();
  atomicWrite(out, render(graph, text, projectDir));
  return `wrote ${out} (${graph.ideas.length} ideas)`;
}
function main(args2) {
  const command = args2[0];
  const projectDir = resolve(flag(args2, "project") ?? cwd());
  const file = resolve(flag(args2, "file") ?? graphPath(projectDir));
  const today = flag(args2, "date") ?? (/* @__PURE__ */ new Date()).toISOString().slice(0, 10);
  const READS_ANY_GRAPH = ["check", "next", "show", "log", "status", "render", "allow", "paths", "scan"];
  if (!READS_ANY_GRAPH.includes(command) && !sameFile(file, graphPath(projectDir))) {
    const rel = (p) => relative(projectDir, p).replaceAll("\\", "/") || p;
    console.error(`\`${command}\` \u4F1A\u6539\u72B6\u6001\uFF0C\u53EA\u80FD\u4F5C\u7528\u5728\u9879\u76EE\u81EA\u5DF1\u7684\u56FE\u4E0A\uFF1A${rel(graphPath(projectDir))} \u2014\u2014 \u9879\u76EE\u7684\u56FE\u53EA\u6709\u4E00\u4EFD\uFF08D10\uFF09\u3002`);
    console.error(`--file ${rel(file)} \u662F\u53E6\u4E00\u4EFD\u56FE\uFF1B\u6279\u51C6\u53E3\u4EE4\u548C\u7EA2\u7EFF\u8BC1\u636E\u90FD\u8BB0\u5728\u9879\u76EE\u56FE\u540D\u4E0B\uFF0C\u5199\u5230\u522B\u5904\u4F1A\u9020\u51FA\u6C38\u8FDC\u7B54\u4E0D\u4E0A\u7684\u53E3\u4EE4\u3002`);
    console.error(`\u53BB\u6389 --file \u91CD\u8DD1\u3002\u53EA\u60F3\u770B\u90A3\u4EFD\u65E7\u56FE\uFF1A${READS_ANY_GRAPH.join(" / ")} \u52A0 --file \u7167\u5E38\u53EF\u7528\uFF1B\u8981\u628A\u5B83\u7684\u5185\u5BB9\u5E76\u8FDB\u6765\uFF1Amigrate\u3002`);
    return 2;
  }
  if (command === "scan") {
    const all = listProjectFiles(projectDir);
    if (args2.includes("--reset") || !existsSync(worklistFile(projectDir))) {
      writeWorklist(projectDir, all);
      console.log(`worklist: ${all.length} \u4E2A\u6587\u4EF6\u5F85\u8BFB \u2192 ${relative(projectDir, worklistFile(projectDir))}`);
    } else if (!existsSync(doneFile(projectDir))) {
      const stillTodo = new Set(readChecklist(projectDir).map((f) => f.toLowerCase()));
      const already = all.filter((f) => !stillTodo.has(f.toLowerCase()));
      writeWorklist(projectDir, all);
      if (already.length > 0) appendFileSync(doneFile(projectDir), already.map((f) => `${f}
`).join(""));
      console.log(`\u8FC1\u79FB\u5230\u53EA\u8FFD\u52A0\u7684\u8BB0\u5F55\uFF1A${all.length} \u4E2A\u6587\u4EF6\uFF0C\u5176\u4E2D ${already.length} \u4E2A\u6B64\u524D\u5DF2\u8BFB`);
    }
    const { added, removed } = reconcileWorklist(projectDir, all);
    if (added.length > 0 || removed.length > 0) {
      console.log(`\u6E05\u5355\u5DF2\u5BF9\u8D26\uFF1A\u65B0\u51FA\u73B0 ${added.length} \u4E2A\uFF08\u672A\u8BFB\uFF09\uFF0C\u6D88\u5931 ${removed.length} \u4E2A`);
      for (const f of added.slice(0, 20)) console.log(`  + ${f}`);
      if (added.length > 20) console.log(`  \u2026 \u53E6\u6709 ${added.length - 20} \u4E2A\u65B0\u6587\u4EF6`);
    }
    const skipped = skippedFiles(projectDir);
    if (skipped.length > 0) {
      console.log(`\u8DF3\u8FC7 ${skipped.length} \u4E2A\u6587\u4EF6${args2.includes("--skipped") ? "\uFF1A" : "\uFF08--skipped \u9010\u6761\u5217\u51FA\uFF0C\u5404\u5E26\u539F\u56E0\uFF09"}`);
      if (args2.includes("--skipped")) for (const s of skipped) console.log(`  - ${s.file}	${s.reason}`);
    }
    const left = readWorklist(projectDir);
    const total = readChecklist(projectDir).length;
    const done = worklistDone(projectDir);
    console.log(`\u5DF2\u8BFB ${done}/${total}${left.length === 0 ? "  \u2014  \u5168\u90E8\u8BFB\u5B8C" : `\uFF0C\u8FD8\u5269 ${left.length}\uFF1A`}`);
    const batch = Number(flag(args2, "n") ?? 40);
    const shown = batch === 0 ? left : left.slice(0, batch);
    for (const file2 of shown) console.log(`  ${file2}`);
    if (shown.length < left.length) console.log(`  \u2026 \u8FD8\u6709 ${left.length - shown.length} \u4E2A\uFF08--n 0 \u770B\u5168\u90E8\uFF09`);
    return 0;
  }
  if (command === "init") {
    if (existsSync(file)) {
      console.log(`already there: ${file}`);
      return 0;
    }
    mkdirSync(dirname(file), { recursive: true });
    atomicWrite(file, SEED.replace("PROJECT_NAME", projectDir.split(/[\\/]/).pop() ?? "project"));
    console.log(`created ${file}`);
    return 0;
  }
  if (command === "migrate") {
    const result = migrate(projectDir, {
      pick: flag(args2, "pick"),
      dryRun: args2.includes("--dry-run"),
      date: today
    });
    for (const line of result.report ?? []) console.log(line);
    if (!result.ok) {
      console.error(result.reason);
      return 1;
    }
    if (result.written) console.log(`
\u5199\u51FA ${relative(projectDir, result.written)}\uFF1B\u62A5\u544A\u5728 ideas/migrate-report.md`);
    return 0;
  }
  if (command === "paths") {
    for (const [name, value] of Object.entries(paths(projectDir))) {
      console.log(`${name}	${relative(projectDir, value).replaceAll("\\", "/")}`);
    }
    return 0;
  }
  const { doc, graph } = load(file);
  switch (command) {
    case "check": {
      const { errors, warnings } = check(graph, projectDir, file);
      for (const w of warnings) console.log(`warn  ${w}`);
      for (const e of errors) console.log(`ERROR ${e}`);
      console.log(`
${graph.ideas.length} ideas \xB7 ${errors.length} errors \xB7 ${warnings.length} warnings`);
      return errors.length > 0 ? 1 : 0;
    }
    case "next": {
      const ready = frontier(graph);
      if (ready.length === 0) {
        const left = graph.ideas.filter((i) => (i.status ?? "todo") !== "done");
        console.log(left.length === 0 ? "everything is done." : "nothing is ready \u2014 every remaining idea is blocked or waiting:");
        for (const i of left) console.log(`  ${i.id}  ${i.name}  [${i.status ?? "todo"}]  needs ${(i.needs ?? []).join(", ") || "\u2014"}`);
        return 0;
      }
      for (const i of ready) console.log(`${i.id}	${i.name}`);
      return 0;
    }
    case "show": {
      const idea = byId(graph).get(args2[1]);
      if (!idea) {
        console.error(`no idea with id ${args2[1]}`);
        return 1;
      }
      const map = byId(graph);
      console.log(`${idea.id}  ${idea.name}  [${idea.status ?? "todo"}]`);
      for (const q of QUESTIONS) {
        const value = q.key === "code" ? (idea.code ?? []).map((c) => `${c.file}${c.lines ? ":" + c.lines : ""}${c.symbol ? ` (${c.symbol})` : ""}`).join(", ") : q.key === "verify" ? idea.verify?.command ?? idea.verify?.manual : idea[q.key];
        console.log(`
${q.n} ${q.label}
  ${(value || "\u2014").trim().replace(/\n/g, "\n  ")}`);
      }
      console.log(`
\u524D\u7F6E\u60F3\u6CD5  ${(idea.needs ?? []).map((n) => `${n} (${map.get(n)?.status ?? "?"})`).join(", ") || "\u2014"}`);
      console.log(`\u5B83\u662F\u8C01\u7684\u524D\u7F6E  ${dependents(graph, idea.id).join(", ") || "\u2014"}`);
      for (const l of idea.log ?? []) console.log(`  log ${l.date} ${l.by ?? ""} ${l.note}`);
      return 0;
    }
    case "log": {
      const wanted = args2[1] && !args2[1].startsWith("--") ? args2[1] : void 0;
      if (wanted && !byId(graph).has(wanted)) {
        console.error(`no idea with id ${wanted}`);
        return 1;
      }
      const wall = wanted ? [byId(graph).get(wanted)] : graph.ideas;
      const tail = Number(flag(args2, "n")) || 0;
      let printed = 0;
      for (const idea of wall) {
        const entries = idea.log ?? [];
        if (entries.length === 0) continue;
        console.log(`${idea.id}	${idea.name}	[${idea.status ?? "todo"}]`);
        for (const l of tail > 0 ? entries.slice(-tail) : entries) {
          console.log(`  ${l.date}	${l.by ?? "\u2014"}	${l.note}`);
          printed += 1;
        }
      }
      if (printed === 0) console.log(wanted ? `${wanted} \u8FD8\u6CA1\u6709\u4EFB\u4F55\u4FEE\u6539\u8BB0\u5F55` : "\u8FD9\u5F20\u56FE\u91CC\u8FD8\u6CA1\u6709\u4EFB\u4F55\u4FEE\u6539\u8BB0\u5F55");
      return 0;
    }
    case "set": {
      setStatus(
        doc,
        graph,
        args2[1],
        args2[2],
        { by: flag(args2, "by"), note: flag(args2, "note"), date: today },
        projectDir
      );
      save(file, doc);
      redraw(file, projectDir);
      console.log(`${args2[1]} \u2192 ${args2[2]}`);
      return 0;
    }
    case "new": {
      const name = args2[1];
      if (!name || name.startsWith("--")) {
        console.error(usageOf("new"));
        return 2;
      }
      const needs = (flag(args2, "needs") ?? "").split(",").map((s) => s.trim()).filter(Boolean);
      const id = addIdea(doc, graph, name, needs, today);
      save(file, doc);
      redraw(file, projectDir);
      console.log(id);
      return 0;
    }
    case "allow": {
      if (!args2[1]) {
        console.error(usageOf("allow"));
        return 2;
      }
      const verdict = decideProductWrite(projectDir, graph, args2[1]);
      console.log(`${verdict.allow ? "allow" : "deny"}	${args2[1]}	${verdict.reason}`);
      return verdict.allow ? 0 : 1;
    }
    case "run-check": {
      const id = args2[1];
      const phase = flag(args2, "phase");
      if (!id || !phase || !["red", "green"].includes(phase)) {
        console.error(usageOf("run-check"));
        return 2;
      }
      const record2 = runCheck(
        projectDir,
        graph,
        id,
        phase,
        { timeoutMs: (Number(flag(args2, "timeout")) || 120) * 1e3 }
      );
      console.log(`${phase} \u2192 \u9000\u51FA\u7801 ${record2.exit_code}${record2.outcome ? ` (${record2.outcome})` : ""}`);
      if (record2.outcome === "infra_error") {
        console.log(`\u9A8C\u8BC1\u547D\u4EE4\u6CA1\u80FD\u771F\u6B63\u8DD1\u8D77\u6765\uFF1A${record2.infra_error} \u2014\u2014 \u8FD9\u4E0D\u662F\u6D4B\u8BD5\u7EA2\u4E86\uFF0C\u5B9E\u73B0\u7684\u95E8\u4E0D\u5F00\u3002`);
        console.log(record2.output_tail);
        return 1;
      }
      if (phase === "red" && record2.outcome !== "unexpected_pass") {
        const gate = redGateReady(projectDir, graph, id);
        if (!gate.ready) console.log(`\u6CE8\u610F\uFF1A${gate.reason}`);
      }
      if (record2.outcome === "unexpected_pass") {
        console.log(`\u6D4B\u8BD5\u8FD8\u6CA1\u5B9E\u73B0\u5C31\u901A\u8FC7\u4E86 \u2014\u2014 \u8FD9\u6321\u4F4F\u5B9E\u73B0\u5199\u5165\u3002\u8981\u4E48\u6D4B\u8BD5\u5199\u9519\u4E86\uFF0C\u8981\u4E48\u771F\u6709\u73B0\u6210\u5B9E\u73B0\uFF1A`);
        console.log(`  request-approval --gate red-waiver --node ${id}   # \u8BF7\u4EBA\u88C1\u51B3`);
      }
      if (phase === "green" && record2.exit_code !== 0) {
        console.log(record2.output_tail);
        return 1;
      }
      return 0;
    }
    case "request-approval": {
      const gate = flag(args2, "gate");
      if (!gate || !["decomposition", "plan", "red-waiver", "manual-check"].includes(gate)) {
        console.error(usageOf("request-approval"));
        return 2;
      }
      const nodes = (flag(args2, "node") ?? "").split(",").map((s) => s.trim()).filter(Boolean);
      const r = requestApproval(
        projectDir,
        graph,
        gate,
        nodes.length ? nodes : void 0,
        { by: flag(args2, "by"), date: today }
      );
      console.log(`\u4E00\u6B21\u6027\u53E3\u4EE4\uFF1A${r.challenge}\uFF08${gate}${nodes.length ? ` \xB7 ${nodes.join(", ")}` : ""}\uFF09`);
      console.log(`\u8BF7\u4EBA\u770B\u8FC7\u5185\u5BB9\u540E\uFF0C\u6574\u6761\u6D88\u606F\u56DE\u590D\uFF1A\u6279\u51C6 ${r.challenge}`);
      console.log(`\uFF08\u62D2\u7EDD\u5C31\u56DE\uFF1A\u62D2\u7EDD ${r.challenge}\u3002\u5185\u5BB9\u6539\u52A8\u6216\u53E3\u4EE4\u7528\u8FC7\u4E00\u6B21\u5373\u4F5C\u5E9F\u3002\uFF09`);
      return 0;
    }
    case "status": {
      for (const idea of graph.ideas) {
        const st = idea.status ?? "todo";
        let note = "";
        if (st === "todo") {
          note = isBuildReady(idea) ?? needsUnmet(idea, graph) ?? "READY";
        } else if (st === "doing") {
          note = `writing: ${claimedFiles(idea).join(", ") || "\u2014"}`;
        }
        console.log(`${idea.id}	[${st}]	${idea.name}${note ? `	${note}` : ""}`);
      }
      return 0;
    }
    case "render": {
      console.log(redraw(file, projectDir));
      return 0;
    }
    case "serve": {
      serve(projectDir, file, {
        port: Number(flag(args2, "port")) || void 0,
        open: !args2.includes("--no-open")
      }).then((live) => {
        console.log(`\u60F3\u6CD5\u56FE\u5F00\u5728 ${live.url}`);
        console.log(`\u5728\u7F51\u9875\u4E0A\u6539\u5B8C\u70B9\u63D0\u4EA4\uFF0C\u6539\u52A8\u76F4\u63A5\u5199\u56DE ${relative(projectDir, file)} \u2014\u2014 \u4E0D\u7528\u518D\u642C\u6587\u4EF6\u3002`);
        console.log(`\u6309 Ctrl-C \u7ED3\u675F\u3002`);
      }).catch((error) => {
        console.error(`\u8D77\u4E0D\u6765\uFF1A${error instanceof Error ? error.message : error}`);
        exit(1);
      });
      return KEEP_RUNNING;
    }
    case "apply": {
      const given = args2[1] && !args2[1].startsWith("--") ? args2[1] : void 0;
      const changeFile = resolve(given ?? join(IDEAS_DIR(projectDir), "changes.json"));
      if (!existsSync(changeFile)) {
        console.error(`\u6CA1\u6709\u627E\u5230\u6539\u52A8\u6587\u4EF6\uFF1A${changeFile}
\uFF08\u7F51\u9875\u63D0\u4EA4\u65F6\u5982\u679C\u6CA1\u6709\u672C\u5730\u670D\u52A1\uFF0C\u6587\u4EF6\u4F1A\u843D\u5728\u4E0B\u8F7D\u76EE\u5F55 \u2014\u2014 \u628A\u5B83\u79FB\u5230 ideas/ \u518D\u8DD1\u4E00\u6B21\uFF09`);
        return 1;
      }
      let envelope;
      try {
        envelope = JSON.parse(readFileSync(changeFile, "utf8"));
      } catch (error) {
        console.error(`\u6539\u52A8\u6587\u4EF6\u4E0D\u662F\u5408\u6CD5\u7684 JSON\uFF1A${error}`);
        return 1;
      }
      const result = applyChanges(readFileSync(file, "utf8"), envelope, today, projectDir);
      if (!result.ok) {
        console.error(`\u62D2\u7EDD\u5199\u56DE\uFF1A${result.reason}

\u6539\u52A8\u6587\u4EF6\u539F\u6837\u7559\u5728 ${changeFile}\uFF0C\u6CA1\u6709\u52A8\u8FC7\u3002`);
        return 1;
      }
      atomicWrite(file, result.text);
      for (const line of result.changed ?? []) console.log(`  ${line}`);
      console.log(`
\u5199\u56DE ${result.changed?.length ?? 0} \u5904\u6539\u52A8 \u2192 ${relative(projectDir, file)}`);
      const written = (0, import_yaml.parseDocument)(result.text).toJSON();
      for (const line of requestSignatures(projectDir, written, result.signRequests ?? [], today)) {
        console.log(`
${line}`);
      }
      const archived = changeFile.replace(/\.json$/, "") + `.applied-${today}.json`;
      try {
        renameSync(changeFile, archived);
        console.log(`\u6539\u52A8\u6587\u4EF6\u5DF2\u5F52\u6863 \u2192 ${relative(projectDir, archived)}`);
      } catch {
        console.log(`\uFF08\u6539\u52A8\u6587\u4EF6\u5F52\u6863\u5931\u8D25\uFF0C\u5B83\u8FD8\u5728 ${changeFile}\uFF09`);
      }
      console.log(redraw(file, projectDir));
      console.log(`
\u56DE\u6D4F\u89C8\u5668\u5237\u65B0\u4E00\u4E0B\u9875\u9762\uFF0C\u518D\u505A\u4E0B\u4E00\u8F6E\u3002`);
      return 0;
    }
    default:
      for (const line of usageLines()) console.error(line);
      return 2;
  }
}
if (argv[1]?.endsWith("ideas.ts")) {
  try {
    const code = main(argv.slice(2));
    if (code !== KEEP_RUNNING) exit(code);
  } catch (error) {
    console.error(String(error instanceof Error ? error.message : error));
    exit(1);
  }
}

// companion/guard.ts
var import_yaml2 = __toESM(require_dist());
import { readFileSync as readFileSync2, existsSync as existsSync2, appendFileSync as appendFileSync2, mkdirSync as mkdirSync2 } from "node:fs";
import { join as join2, resolve as resolve2, dirname as dirname2 } from "node:path";
import { platform as platform2 } from "node:process";
var OK = { allow: true };
var norm = (p) => p.replaceAll("\\", "/").replace(/\/+$/, "");
var sameFile2 = (a, b) => platform2 === "win32" ? norm(a).toLowerCase() === norm(b).toLowerCase() : norm(a) === norm(b);
function relTo(projectDir, filePath) {
  const root = norm(resolve2(projectDir));
  const full = norm(resolve2(projectDir, filePath));
  const hit = platform2 === "win32" ? full.toLowerCase().startsWith(root.toLowerCase() + "/") : full.startsWith(root + "/");
  return hit ? full.slice(root.length + 1) : full;
}
var ROOT_WALK_LIMIT = 64;
function projectRoot(reported) {
  const start = resolve2(reported);
  let dir = start;
  for (let depth = 0; depth < ROOT_WALK_LIMIT; depth++) {
    if (existsSync2(graphPath(dir))) return dir;
    if (existsSync2(join2(dir, ".git"))) return dir;
    const parent = dirname2(dir);
    if (parent === dir) break;
    dir = parent;
  }
  return start;
}
function decide(event, projectDir, opts = {}) {
  if (opts.guardOff) {
    return { allow: true, warn: "AIDEV_GUARD=off \u2014 \u4E03\u6761\u89C4\u5219\u5168\u90E8\u505C\u7528\u3002\u8FD9\u662F\u9003\u751F\u53E3\uFF0C\u4E0D\u662F\u5E38\u6001\uFF1B\u8BB0\u5F55\u91CC\u4F1A\u5199\u660E\u5F3A\u5236\u5F53\u65F6\u662F\u5173\u7740\u7684\u3002" };
  }
  try {
    return decideInner(event, projectDir);
  } catch (error) {
    const what = error instanceof Error ? error.message : String(error);
    if (event.event === "post-write") {
      return { allow: true, warn: `\u5199\u540E\u8BB0\u5F55\u5931\u8D25\uFF08${what}\uFF09\u2014\u2014 \u5DF2\u5B8C\u6210\u7684\u5199\u4E0D\u88AB\u8FFD\u62E6\uFF08D9\uFF09\uFF0C\u4F46\u8FD9\u7B14\u6CA1\u8BB0\u4E0A\u3002` };
    }
    if (error instanceof MissingGraph) return { allow: false, reason: what };
    return { allow: false, reason: `\u5B88\u536B\u81EA\u8EAB\u51FA\u9519\uFF08${what}\uFF09\u2014\u2014 \u5199\u524D\u73AF\u8282\u6309 D9 \u62E6\u4E0B\u3002\u771F\u88AB\u5361\u6B7B\u65F6\u7684\u9003\u751F\u53E3\uFF1AAIDEV_GUARD=off\uFF08\u4F1A\u88AB\u9192\u76EE\u8BB0\u5F55\uFF09\u3002` };
  }
}
function decideInner(event, projectDir) {
  switch (event.event) {
    case "post-write":
      return OK;
    // recording lives in record()
    case "prompt":
      return OK;
    // approval consumption lives in the adapter (I-094)
    case "read":
      return OK;
    // R7's striking is a side effect, never a verdict
    case "session":
      return OK;
    // the briefing is a message, not a verdict
    case "stop":
      return ruleStop(event, projectDir);
    case "shell":
      return ruleShell(event, projectDir);
    case "pre-write":
      return rulePreWrite(event, projectDir);
    default:
      return OK;
  }
}
var MissingGraph = class extends Error {
};
function loadGraphStrict(projectDir) {
  if (!existsSync2(graphPath(projectDir))) {
    throw new MissingGraph("\u8FD8\u6CA1\u6709\u60F3\u6CD5\u56FE\uFF08ideas/graph.yaml\uFF09\u2014\u2014 \u5148 init \u6216 migrate\u3002\u6CA1\u56FE\u5C31\u6CA1\u6709\u6388\u6743\uFF0C\u9ED8\u8BA4\u62D2\u7EDD\uFF08D16\uFF09");
  }
  return load(graphPath(projectDir)).graph;
}
function rulePreWrite(event, projectDir) {
  const targets = [
    ...event.paths ?? [],
    ...(event.operations ?? []).map((op) => op.path)
  ];
  if (event.unknownTarget || targets.length === 0) {
    return { allow: false, reason: "\u8FD9\u4E2A\u8C03\u7528\u53EF\u80FD\u5199\u6587\u4EF6\uFF0C\u4F46\u770B\u4E0D\u51FA\u5199\u5230\u54EA \u2014\u2014 \u4E25\u683C\u6A21\u5F0F\u4E0B unknown \u4E0D\u7B49\u4E8E allowed\uFF08D23\uFF09\u3002\u7528\u80FD\u5E26\u51FA\u6587\u4EF6\u8DEF\u5F84\u7684\u5DE5\u5177\uFF0C\u6216\u8D70 companion CLI\u3002" };
  }
  const graph = loadGraphStrict(projectDir);
  for (const target of targets) {
    const verdict = decideOnePath(event, graph, projectDir, target);
    if (!verdict.allow) return verdict;
  }
  return OK;
}
function decideOnePath(event, graph, projectDir, target) {
  if (sameFile2(resolve2(projectDir, target).replaceAll("\\", "/"), paths(projectDir).graph.replaceAll("\\", "/"))) {
    return ruleGraphEdit(event, projectDir);
  }
  const verdict = decideProductWrite(projectDir, graph, target);
  return verdict.allow ? OK : { allow: false, reason: verdict.reason };
}
var INITIAL_STATUS = "todo";
function ruleGraphEdit(event, projectDir) {
  const file = graphPath(projectDir);
  if (!existsSync2(file)) return OK;
  const current = readFileSync2(file, "utf8");
  const next = event.edit ? afterEdit(current, event.edit) : null;
  if (next === null) return ruleGraphPatch(event, projectDir, current);
  return compareGraphNodes(current, next);
}
function compareGraphNodes(current, next) {
  const before = graphNodes(current);
  const after = graphNodes(next);
  const seen = /* @__PURE__ */ new Map();
  for (const node of after) if (node.id) seen.set(node.id, (seen.get(node.id) ?? 0) + 1);
  const doubled = [...seen].filter(([, count]) => count > 1).map(([id]) => id);
  if (doubled.length > 0) {
    return {
      allow: false,
      reason: `\u6539\u5B8C\u4E4B\u540E\u60F3\u6CD5\u56FE\u91CC\u6709\u7F16\u53F7\u91CD\u590D\u7684\u60F3\u6CD5\uFF08${doubled.join(", ")}\uFF09\u2014\u2014 \u5F15\u64CE\u662F\u6309\u6570\u7EC4\u4E00\u4E2A\u4E2A\u8BFB\u7684\uFF0C\u540C\u53F7\u7684\u4E24\u4E2A\u8282\u70B9\u90FD\u7B97\u6570\u3001\u90FD\u80FD\u6388\u6743\uFF0C\u6240\u4EE5\u8FD9\u662F\u574F\u56FE\uFF0C\u4E0D\u662F\u300C\u540E\u9762\u90A3\u4E2A\u8BF4\u4E86\u7B97\u300D\uFF08R2/D19/D28\uFF09\u3002\u628A\u591A\u51FA\u6765\u7684\u90A3\u4EFD\u5220\u6389\uFF1B\u65B0\u5F00\u60F3\u6CD5\u7528 new\uFF0C\u7F16\u53F7\u7531 next_id \u53D1\u3002`
    };
  }
  const beforeStatus = /* @__PURE__ */ new Map();
  const beforeSigned = /* @__PURE__ */ new Map();
  for (const node of before) {
    if (!node.id) continue;
    addTo(beforeStatus, node.id, node.status);
    addTo(beforeSigned, node.id, node.signed);
  }
  const flipped = after.filter((n) => n.id && beforeStatus.has(n.id) && !beforeStatus.get(n.id).has(n.status));
  if (flipped.length > 0) {
    return {
      allow: false,
      reason: `\u4E0D\u80FD\u624B\u6539\u5DF2\u6709\u60F3\u6CD5\u7684 status\uFF08${flipped.map((n) => `${n.id}: ${[...beforeStatus.get(n.id)].join("/")} \u2192 ${n.status}`).join("; ")}\uFF09\u2014\u2014 \u7528 set\uFF0C\u5B83\u4F1A\u6821\u9A8C\u8F6C\u79FB\u8868\u3001\u5C31\u7EEA\u6761\u4EF6\u548C\u8BC1\u636E\uFF08R2/D19\uFF09\u3002`
    };
  }
  const inserted = after.filter((n) => (!n.id || !beforeStatus.has(n.id)) && n.status !== INITIAL_STATUS);
  if (inserted.length > 0) {
    return {
      allow: false,
      reason: `\u65B0\u52A0\u7684\u60F3\u6CD5\u53EA\u80FD\u4EE5 ${INITIAL_STATUS} \u843D\u5730\uFF08${inserted.map((n) => `${n.id ?? "\uFF08\u8FD9\u4E2A\u8282\u70B9\u8FDE id \u90FD\u6CA1\u5199\uFF09"}: ${n.status}`).join("; ")}\uFF09\u2014\u2014 \u76F4\u63A5\u5199\u6210\u522B\u7684\u72B6\u6001\u5C31\u662F\u7ED5\u5F00\u8F6C\u79FB\u8868\uFF1A\u65B0\u5F00\u60F3\u6CD5\u7528 new\uFF08\u7F16\u53F7\u7531 next_id \u53D1\uFF09\uFF0C\u518D\u7528 set \u63A8\u8FDB\uFF08R2/D19/D28\uFF09\u3002`
    };
  }
  const NO_SIGNATURE = /* @__PURE__ */ new Set(["null"]);
  const afterIds = new Set(after.map((n) => n.id).filter(Boolean));
  const signed = [
    ...after.filter((n) => !(n.id ? beforeSigned.get(n.id) ?? NO_SIGNATURE : NO_SIGNATURE).has(n.signed)).map((n) => n.id ?? "\uFF08\u8FD9\u4E2A\u8282\u70B9\u8FDE id \u90FD\u6CA1\u5199\uFF09"),
    ...[...beforeSigned.keys()].filter((id) => !afterIds.has(id))
  ];
  if (signed.length > 0) {
    return {
      allow: false,
      reason: `\u4EBA\u5DE5\u9A8C\u6536\u7684\u7B7E\u5B57\uFF08${signed.join(", ")} \u7684 verify.signed_off\uFF09\u53EA\u80FD\u7ECF manual-check \u53E3\u4EE4\u4EA7\u751F\uFF0Cagent \u4E0D\u80FD\u4EE3\u7B7E\uFF08D27\uFF09\u2014\u2014 request-approval --gate manual-check --node ${signed[0]}\u3002`
    };
  }
  return OK;
}
var addTo = (into, key, value) => (into.get(key) ?? into.set(key, /* @__PURE__ */ new Set()).get(key)).add(value);
var PATCH_MARKER = /^([+-])(?!\1)/;
var PATCH_FIELD_HEAD = /^[ \t]*(?:-[ \t]+)*(["']?)(status|signed_off)\1[ \t]*:/;
var PATCH_FIELD_FLOW = /\{(?:[^}]*,)?[ \t]*(["']?)(status|signed_off)\1[ \t]*:/;
function patchFieldOn(line) {
  if (!PATCH_MARKER.test(line)) return null;
  const body = line.slice(1);
  const hit = PATCH_FIELD_HEAD.exec(body) ?? PATCH_FIELD_FLOW.exec(body);
  return hit ? hit[2] : null;
}
var IS_CODEX_PATCH = /^\*{3} (?:Add|Update|Delete) File: /m;
var CODEX_FILE = /^\*{3} (?:Add|Update|Delete) File: (.+?)\s*$/;
var CODEX_MOVE = /^\*{3} Move to: (.+?)\s*$/;
var DIFF_GIT = /^diff --git\s+(\S+)\s+(\S+)\s*$/;
var DIFF_OLD = /^--- (.+)$/;
var DIFF_NEW = /^\+\+\+ (.+)$/;
function patchPathOf(raw) {
  return raw.trim().replace(/\t.*$/, "").replace(/^["']|["']$/g, "").replaceAll("\\", "/").replace(/^\.\//, "").replace(/^[ab]\//, "");
}
var claimedPaths = (...raw) => raw.map(patchPathOf).filter((p) => p !== "" && p !== "/dev/null");
function patchLines(patchText) {
  const lines = patchText.split(/\r?\n/);
  if (lines.at(-1) === "") lines.pop();
  return lines;
}
function fileHunk(patchText, projectDir, rel) {
  const lines = patchLines(patchText);
  const codex = IS_CODEX_PATCH.test(patchText);
  const mine = [];
  let current = null;
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];
    if (codex) {
      const file = CODEX_FILE.exec(line);
      if (file) {
        current = claimedPaths(file[1]);
        continue;
      }
      const move = CODEX_MOVE.exec(line);
      if (move) {
        current = [...current ?? [], ...claimedPaths(move[1])];
        continue;
      }
    } else {
      const git = DIFF_GIT.exec(line);
      if (git) {
        current = claimedPaths(git[1], git[2]);
        continue;
      }
      const old = DIFF_OLD.exec(line);
      const next = old ? DIFF_NEW.exec(lines[i + 1] ?? "") : null;
      if (old && next) {
        current = claimedPaths(old[1], next[1]);
        i++;
        continue;
      }
    }
    if (current === null || current.some((p) => sameFile2(relTo(projectDir, p), rel))) mine.push(line);
  }
  return mine;
}
var HUNK_HEAD = /^@@+[ \t]+-(\d+)(?:,(\d+))?/;
function hunkStart(line) {
  const hit = HUNK_HEAD.exec(line);
  if (!hit) return null;
  const count = hit[2] === void 0 ? 1 : Number(hit[2]);
  return count === 0 ? Number(hit[1]) : Math.max(Number(hit[1]) - 1, 0);
}
function patchHunks(lines) {
  const hunks = [];
  for (const line of lines) {
    if (line.startsWith("@@")) {
      hunks.push({ before: [], after: [], at: hunkStart(line) });
      continue;
    }
    if (line.startsWith("***") || line.startsWith("\\")) continue;
    const mark = line[0] ?? "";
    if (hunks.length === 0) {
      if (mark !== "+" && mark !== "-") continue;
      hunks.push({ before: [], after: [], at: null });
    }
    const hunk = hunks[hunks.length - 1];
    if (mark === "+") {
      hunk.after.push(line.slice(1));
      continue;
    }
    if (mark === "-") {
      hunk.before.push(line.slice(1));
      continue;
    }
    const context = mark === " " ? line.slice(1) : line;
    hunk.before.push(context);
    hunk.after.push(context);
  }
  return hunks;
}
function locateHunk(lines, hunk, from) {
  if (hunk.before.length === 0) return hunk.at === null ? null : Math.min(hunk.at, lines.length);
  for (const loose of [false, true]) {
    for (let i = from; i + hunk.before.length <= lines.length; i++) {
      const fits = hunk.before.every((want, k) => loose ? lines[i + k].trimEnd() === want.trimEnd() : lines[i + k] === want);
      if (fits) return i;
    }
  }
  return null;
}
function applyPatchHunks(current, lines) {
  const hunks = patchHunks(lines);
  if (hunks.length === 0) return current;
  const eol = current.includes("\r\n") ? "\r\n" : "\n";
  const out = current.split(/\r?\n/);
  let from = 0;
  for (const hunk of hunks) {
    const at = locateHunk(out, hunk, from);
    if (at === null) return null;
    out.splice(at, hunk.before.length, ...hunk.after);
    from = at + hunk.after.length;
  }
  return out.join(eol);
}
function ruleGraphPatch(event, projectDir, current) {
  if (event.patchText === void 0) {
    return {
      allow: false,
      reason: `${event.tool ?? "\u8FD9\u6B21\u8C03\u7528"} \u8981\u5199\u60F3\u6CD5\u56FE\uFF0C\u5374\u5E26\u4E0D\u51FA\u6539\u52A8\u540E\u7684\u5185\u5BB9 \u2014\u2014 \u770B\u4E0D\u89C1\u5C31\u8BC1\u660E\u4E0D\u4E86 status \u548C signed_off \u6CA1\u88AB\u52A8\uFF0C\u4E25\u683C\u6A21\u5F0F\u4E0B\u62D2\u7EDD\uFF08R2/D24\uFF09\u3002\u6539\u72B6\u6001\u7528 set\uFF1B\u4EBA\u5DE5\u9A8C\u6536\u7B7E\u5B57\u8D70 request-approval --gate manual-check\uFF1B\u53EA\u6539\u53D9\u8FF0\u8BF7\u7528\u5E26\u5F97\u51FA\u6539\u52A8\u5185\u5BB9\u7684\u7F16\u8F91\u5DE5\u5177\u3002`
    };
  }
  const rel = relTo(projectDir, graphPath(projectDir));
  const lines = fileHunk(event.patchText, projectDir, rel);
  const next = applyPatchHunks(current, lines);
  if (next !== null && parsesAsGraph(next)) {
    const verdict = compareGraphNodes(current, next);
    if (!verdict.allow) return { ...verdict, reason: `\u8865\u4E01\u6539\u7684\u662F\u60F3\u6CD5\u56FE ${rel}\uFF1A${verdict.reason}` };
    for (const line of lines) {
      if (patchFieldOn(line) !== "signed_off") continue;
      return {
        allow: false,
        reason: `\u8865\u4E01\u91CC\u76F4\u63A5\u589E\u5220\u4E86\u60F3\u6CD5\u56FE ${rel} \u7684 signed_off\uFF08\u300C${line.trim().slice(0, 60)}\u300D\uFF09\u2014\u2014 \u4EBA\u5DE5\u9A8C\u6536\u7684\u7B7E\u5B57\u53EA\u80FD\u7ECF manual-check \u53E3\u4EE4\u4EA7\u751F\uFF0Cagent \u4E0D\u80FD\u4EE3\u7B7E\uFF08D27\uFF09\uFF1Arequest-approval --gate manual-check --node <\u60F3\u6CD5\u7F16\u53F7>\u3002`
      };
    }
    return verdict;
  }
  for (const line of lines) {
    const field = patchFieldOn(line);
    if (!field) continue;
    return field === "status" ? {
      allow: false,
      reason: `\u8865\u4E01\u91CC\u76F4\u63A5\u589E\u5220\u4E86\u60F3\u6CD5\u56FE ${rel} \u7684 status\uFF08\u300C${line.trim().slice(0, 60)}\u300D\uFF09\uFF0C\u800C\u4E14\u8FD9\u4E2A\u8865\u4E01\u8D34\u4E0D\u56DE\u73B0\u5728\u7684\u6587\u4EF6 \u2014\u2014 \u6539\u72B6\u6001\u7528 set\uFF0C\u5B83\u4F1A\u6821\u9A8C\u8F6C\u79FB\u8868\u3001\u5C31\u7EEA\u6761\u4EF6\u548C\u8BC1\u636E\uFF1B\u65B0\u5F00\u4E00\u4E2A\u60F3\u6CD5\u7528 new\uFF0C\u7F16\u53F7\u7531 next_id \u53D1\uFF08R2/D19/D28\uFF09\u3002`
    } : {
      allow: false,
      reason: `\u8865\u4E01\u91CC\u76F4\u63A5\u589E\u5220\u4E86\u60F3\u6CD5\u56FE ${rel} \u7684 verify.signed_off\uFF08\u300C${line.trim().slice(0, 60)}\u300D\uFF09\uFF0C\u800C\u4E14\u8FD9\u4E2A\u8865\u4E01\u8D34\u4E0D\u56DE\u73B0\u5728\u7684\u6587\u4EF6 \u2014\u2014 \u4EBA\u5DE5\u9A8C\u6536\u7684\u7B7E\u5B57\u53EA\u80FD\u7ECF manual-check \u53E3\u4EE4\u4EA7\u751F\uFF0Cagent \u4E0D\u80FD\u4EE3\u7B7E\uFF08D27\uFF09\uFF1Arequest-approval --gate manual-check --node <\u60F3\u6CD5\u7F16\u53F7>\u3002`
    };
  }
  return {
    allow: false,
    reason: `\u8865\u4E01\u8981\u6539\u60F3\u6CD5\u56FE ${rel}\uFF0C\u4F46\u5B83\u7684 hunk \u8D34\u4E0D\u56DE\u73B0\u5728\u7684\u6587\u4EF6\uFF08\u4E0A\u4E0B\u6587\u5BF9\u4E0D\u4E0A\uFF0C\u6216\u8005\u53EA\u6709\u589E\u884C\u3001\u6CA1\u8BF4\u52A0\u5728\u54EA\uFF09\u2014\u2014 \u91CD\u5EFA\u4E0D\u51FA\u6539\u52A8\u540E\u7684\u56FE\uFF0C\u5C31\u8BC1\u660E\u4E0D\u4E86\u8FD9\u4E00\u6539\u6CA1\u6709\u5077\u52A0\u60F3\u6CD5\u3001\u6CA1\u6709\u7FFB\u72B6\u6001\u3001\u6CA1\u6709\u4EE3\u7B7E\uFF0C\u91CD\u5EFA\u4E0D\u51FA\u6765\u7684\u56FE\u5199\u6309 D23 \u62D2\u7EDD\uFF0C\u4E0D\u653E\u884C\u3002\u5148\u628A ${rel} \u91CD\u65B0\u8BFB\u4E00\u904D\u518D\u51FA\u8865\u4E01\uFF1B\u6539\u72B6\u6001\u7528 set\uFF0C\u4EBA\u5DE5\u9A8C\u6536\u7B7E\u5B57\u8D70 request-approval --gate manual-check\u3002`
  };
}
function afterEdit(current, edit) {
  if (edit.content !== void 0) return edit.content;
  if (edit.old_string === void 0 || edit.new_string === void 0) return null;
  return edit.replace_all ? current.split(edit.old_string).join(edit.new_string) : current.replace(edit.old_string, edit.new_string);
}
function parsesAsGraph(text) {
  try {
    (0, import_yaml2.parseDocument)(text).toJSON();
    return true;
  } catch {
    return false;
  }
}
function graphNodes(text) {
  try {
    const graph = (0, import_yaml2.parseDocument)(text).toJSON();
    return (graph?.ideas ?? []).map((idea) => ({
      id: idea?.id ?? null,
      status: idea?.status ?? INITIAL_STATUS,
      signed: JSON.stringify(idea?.verify?.signed_off ?? null)
    }));
  } catch {
  }
  return [];
}
var GIT_GLOBAL = String.raw`(-[Cc][ \t]+\S+|--(git-dir|work-tree|namespace|exec-path|config-env)(=\S+|[ \t]+\S+)|--?[\w-]+(=\S+)?)[ \t]+`;
var MUTATING_HEAD = new RegExp([
  String.raw`^(rm|del|erase|rmdir|rd|mv|move|cp|copy|xcopy|robocopy|ren|rename|tee|touch)(\.exe)?\b`,
  // The quieter half of the copy family, each measured against the same
  // question: standing where the program goes, does it land a file? `dd` and
  // `install` copy one, `ln` / `link` / `mklink` create one, and `truncate`,
  // `shred`, `patch` and `Clear-Content` rewrite one where it stands. The last
  // four were on ENGINE_WRITE, i.e. refused only when they stood next to the
  // engine's own files — nothing about them is engine-specific, and
  // `patch -p1 < evil.diff` writes whatever the diff names. `find` stays down
  // there on purpose: at a head it is a search (D21).
  String.raw`^(dd|install|ln|link|mklink|truncate|shred|patch|Clear-Content)(\.exe)?\b`,
  // Unpacking an archive is a bulk write whose destinations are chosen by the
  // archive rather than by the command line: `tar -xf p.tar` lands every path
  // stored inside it. Named as whole verbs, with the cost stated the way the
  // downloader line below states its own — `tar -tf` and `unzip -l` only LIST,
  // and are refused with them; listing an archive is a read tool's job (D21).
  String.raw`^(tar|bsdtar|unzip|unar|unrar|7z|7za|7zr|gzip|gunzip|bzip2|bunzip2|xz|unxz|zstd|unzstd|cpio|Expand-Archive|Compress-Archive)(\.exe)?\b`,
  String.raw`^(sed|perl)(\.exe)?\b[^\n]*[ \t]-i\b`,
  // Every git subcommand that writes the WORKING TREE, not just the four that
  // rewrite tracked content. The missing ones were ordinary ways to overwrite
  // any file in the project: `git switch`/`git stash`/`git rebase`/`git pull`
  // rewrite it from history, `git rm` deletes it, `git am`/`git cherry-pick`
  // apply a patch exactly as `git apply` does, and `git clone`/`git init`/
  // `git worktree`/`git submodule` create trees (D21). `add`, `diff`, `log`,
  // `show`, `status` and `blame` stay off the list: staging and reading are the
  // ordinary work this screen must not touch.
  String.raw`^git(\.exe)?[ \t]+(${GIT_GLOBAL})*(am|apply|checkout|cherry-pick|clean|clone|commit|filter-branch|format-patch|init|merge|mv|pull|rebase|reset|restore|revert|rm|sparse-checkout|stash|submodule|switch|worktree)\b`,
  String.raw`^(npm|pnpm|yarn|pip|pip3|poetry)(\.exe)?[ \t]+(add|install|remove|uninstall|update)\b`,
  String.raw`^(Set-Content|Add-Content|Out-File|New-Item|Remove-Item|Move-Item|Copy-Item|Rename-Item)\b`,
  // Downloaders. A fetch that lands a file is a write, and it was the one write
  // shape with no verb on this list: `curl -o src/a.ts …` and `wget …` walk in
  // past the path check exactly like `cp` would, and only the piped-into-a-shell
  // spelling was ever caught (by INTERPRETER below). Named as whole verbs, the
  // way the copy family above is; the cost, stated: a read-only `curl` that
  // prints a URL to stdout is refused with them — reading a URL is the agent's
  // own fetch tool's job, not a shell write's (D21).
  String.raw`^(curl|wget|aria2c|scp|rsync|iwr|irm|Invoke-WebRequest|Invoke-RestMethod|Start-BitsTransfer)(\.exe)?\b`
].join("|"), "i");
var DOWNLOADER_HEAD = /^(curl|wget|aria2c|scp|rsync|iwr|irm|Invoke-)/i;
var GIT_COMMIT_HEAD = /^git(\.exe)?\b.*\bcommit$/i;
var REDIRECT = /(?:^|[^<])(<>|>{1,2})/;
var REDIRECT_TARGET = /(?:<>|>{1,2})[ \t]*("[^"\r\n]*"|'[^'\r\n]*'|[^\s;&|<>]+)/g;
var isRedirectToken = (token) => /^(?:<>|>{1,2})$/.test(token);
var TOKEN = /(?:"[^"]*"|'[^']*'|[^\s])+/g;
var HANDOFF = /^(sudo|npx|bunx|command|env|exec|time|nohup|xargs)$/i;
var HANDOFF_FLAG = /^-(exec|execdir|ok|okdir)$/i;
var ENV_ASSIGN = /^[A-Za-z_]\w*=/;
function shellStages(line) {
  const stages = [];
  let current = "";
  let quote = null;
  for (let i = 0; i < line.length; i++) {
    const ch = line[i];
    if (quote) {
      if (ch === quote) quote = null;
      current += ch;
    } else if (ch === '"' || ch === "'") {
      quote = ch;
      current += ch;
    } else if (ch === "$" && line[i + 1] === "(") {
      stages.push(current);
      current = "";
      i++;
    } else if (";&|()`\r\n".includes(ch)) {
      stages.push(current);
      current = "";
    } else {
      current += ch;
    }
  }
  stages.push(current);
  return stages;
}
function shellCommands(line) {
  const commands = [];
  for (const stage of shellStages(line)) {
    const tokens = stage.match(TOKEN) ?? [];
    let head = true;
    let afterFlag = false;
    tokens.forEach((token, i) => {
      if (HANDOFF_FLAG.test(token)) {
        head = true;
        afterFlag = false;
        return;
      }
      if (token.startsWith("-")) {
        afterFlag ||= head;
        return;
      }
      if (ENV_ASSIGN.test(token)) return;
      if (head && HANDOFF.test(token)) {
        afterFlag = false;
        return;
      }
      if (!head) return;
      commands.push(tokens.slice(i).join(" "));
      head = afterFlag;
      afterFlag = false;
    });
  }
  return commands;
}
function mutatingShell(line) {
  for (const command of shellCommands(line)) {
    const hit = MUTATING_HEAD.exec(command);
    if (hit) return hit[0].trim().slice(0, 40);
  }
  return REDIRECT.exec(line)?.[1] ?? null;
}
function mutatingReason(command, token) {
  const what = isRedirectToken(token) ? `\u300C${token}\u300D\u662F\u91CD\u5B9A\u5411\uFF0C\u843D\u5730\u7684\u662F\u6587\u4EF6` : GIT_COMMIT_HEAD.test(token) ? `\u300C${token}\u300D\u7AD9\u5728\u547D\u4EE4\u5934\u4E0A\uFF0C\u5B83\u628A\u6539\u52A8\u8BB0\u8FDB\u5386\u53F2` : `\u300C${token}\u300D\u7AD9\u5728\u547D\u4EE4\u5934\u4E0A\uFF0C\u5B83\u5199\u6587\u4EF6`;
  const remedy = isRedirectToken(token) ? "\u8981\u5199\u5C31\u7528\u7F16\u8F91\u5DE5\u5177\uFF08\u4F1A\u7ECF\u5B88\u536B\u68C0\u67E5\uFF09\uFF1B\u53EA\u60F3\u770B\u8F93\u51FA\uFF0C\u628A\u91CD\u5B9A\u5411\u53BB\u6389\uFF0C\u6539\u6210\u7BA1\u9053\u63A5 head / less / Select-Object\u3002" : GIT_COMMIT_HEAD.test(token) ? "\u63D0\u4EA4\u8FD9\u4E00\u6B65\u5F52\u4EBA\uFF1A\u628A\u6539\u4E86\u4EC0\u4E48\u3001\u4E3A\u4EC0\u4E48\u6539\u8BF4\u6E05\u695A\uFF0C\u8BF7\u4EBA\u81EA\u5DF1\u6572\u8FD9\u4E00\u6761 git commit\u3002\u522B\u6362\u4E2A\u52A8\u8BCD\u628A\u5B83\u505A\u51FA\u6765 \u2014\u2014 \u90A3\u53EA\u662F\u628A\u8FD9\u4E00\u6B65\u4ECE\u4EBA\u773C\u524D\u632A\u8D70\u3002\u6BCF\u6B21\u5199\u6587\u4EF6\u5B88\u536B\u90FD\u5DF2\u7ECF\u8BB0\u8FDB\u8D26\u672C\uFF08R1\uFF09\uFF0C\u4E0D\u63D0\u4EA4\u4E5F\u4E0D\u4F1A\u4E22\u3002\u89C9\u5F97\u63D0\u4EA4\u672C\u5C31\u8BE5\u653E\u884C\uFF0C\u90A3\u662F\u89C4\u8303\u7684\u4E8B\uFF1AFORMAT.md \u628A commit \u5217\u8FDB\u4E86\u5199\u76D8\u7684 git \u5B50\u547D\u4EE4\uFF0C\u53BB\u6539\u90A3\u6761\u88C1\u51B3\uFF0C\u4E0D\u662F\u7ED5\u8FD9\u9053\u95F8\u3002" : DOWNLOADER_HEAD.test(token) ? "\u4ECE\u7F51\u4E0A\u62C9\u4E1C\u897F\u843D\u5730\u4E5F\u662F\u5199\uFF1A\u5148\u8BA9\u4EBA\u770B\u8FC7\u5185\u5BB9\uFF0C\u518D\u7531\u7F16\u8F91\u5DE5\u5177\u5199\u8FDB\u6765\u3002" : "\u6539\u4EA7\u54C1\u6587\u4EF6\u7528\u7F16\u8F91\u5DE5\u5177\uFF08\u4F1A\u7ECF\u5B88\u536B\u68C0\u67E5\uFF09\uFF1B\u8DD1\u6D4B\u8BD5\u7528 run-check\u3002";
  return `${what} \u2014\u2014 shell \u91CC\u7684\u5199\u6587\u4EF6\u62DB\u6570\u548C\u6587\u4EF6\u5DE5\u5177\u8D70\u540C\u4E00\u9053\u95F8\uFF08D21\uFF09\uFF1A\u300C${command.slice(0, 80)}\u300D\u88AB\u62E6\u3002${remedy}\u8FD9\u9053\u95F8\u53EA\u8BA4\u547D\u4EE4\u5934\uFF0C\u4E0D\u8BA4\u53C2\u6570\u91CC\u51FA\u73B0\u8FC7\u7684\u8BCD\uFF1A\u641C\u5B83\u3001\u8BFB\u5B83\u3001diff \u5B83\u3001\u628A\u540D\u5B57\u91CC\u5E26\u5B83\u7684\u6587\u4EF6\u52A0\u8FDB\u6682\u5B58\u533A\uFF0C\u90FD\u7167\u5E38\u653E\u884C\uFF08rg "Out-File" companion/guard.ts\u3001rg -n touch companion/guard.ts\u3001git diff rename-plan.md\uFF09\u3002`;
}
function protectedTarget(projectDir, token) {
  const eq = token.indexOf("=");
  if (eq > 0) {
    const tail = protectedTarget(projectDir, token.slice(eq + 1));
    if (tail) return tail;
  }
  const bare = token.replace(/^["']|["']$/g, "");
  if (bare === "") return null;
  const p = paths(projectDir);
  const full = resolve2(projectDir, bare).replaceAll("\\", "/");
  for (const [file, label] of [
    [p.approved, "\u6279\u51C6\u56DE\u6267"],
    [p.worklist, "\u626B\u63CF\u6E05\u5355"],
    [p.done, "\u5DF2\u8BFB\u8BB0\u5F55"],
    [p.html, "\u751F\u6210\u7684\u7F51\u9875"],
    [p.graph, "\u60F3\u6CD5\u56FE"]
  ]) {
    if (sameFile2(full, file.replaceAll("\\", "/"))) return label;
  }
  const rel = relTo(projectDir, bare);
  const relLower = platform2 === "win32" ? rel.toLowerCase() : rel;
  if (relLower === "ideas/.runtime" || relLower.startsWith("ideas/.runtime/")) return "\u8FD0\u884C\u671F\u8BC1\u636E";
  if (/^ideas\/graph\.[^/]+\.ya?ml$/i.test(rel)) return "\u8FC1\u79FB\u7528\u7684\u65E7\u56FE\uFF08\u8FC1\u79FB\u540E\u53EA\u8BFB\uFF09";
  return null;
}
var LOOKING_HEAD = new RegExp([
  String.raw`^(cat|bat|tac|nl|head|tail|less|more|type|od|xxd|hexdump|Format-Hex)$`,
  String.raw`^(wc|du|stat|file|ls|dir|tree|Get-ChildItem|gci|Get-Item|gi|Test-Path|Get-FileHash)$`,
  String.raw`^(Get-Content|gc|Select-String|sls|Select-Object|Measure-Object|Compare-Object)$`,
  String.raw`^(rg|grep|egrep|fgrep|ack|ag|findstr|awk|sed|jq|yq|sort|uniq|cut|column|tr)$`,
  String.raw`^(diff|cmp|delta|code|git|md5sum|sha1sum|sha256sum|sha512sum|shasum|cksum)$`,
  String.raw`^(basename|dirname|realpath|readlink|start|open|xdg-open|explorer|Invoke-Item|ii)$`
].join("|"), "i");
function protectedTargetRefusal(command, projectDir) {
  const hits = [];
  for (const part of shellCommands(command)) {
    const tokens = part.match(TOKEN) ?? [];
    const head = (tokens[0] ?? "").replace(/^["']|["']$/g, "").replace(/\.exe$/i, "");
    if (LOOKING_HEAD.test(head)) continue;
    for (const token2 of tokens.slice(1)) {
      const label2 = protectedTarget(projectDir, token2);
      if (label2) hits.push([label2, token2]);
    }
  }
  for (const hit of command.matchAll(REDIRECT_TARGET)) {
    const label2 = protectedTarget(projectDir, hit[1]);
    if (label2) hits.push([label2, hit[1]]);
  }
  if (hits.length === 0) return null;
  const [label, token] = hits[0];
  return `\u300C${token}\u300D\u662F${label} \u2014\u2014 \u8FD9\u4E00\u4EFD\u53EA\u80FD\u7531 CLI \u548C hook \u5199\uFF08D24\uFF09\uFF0C\u6240\u4EE5\u5B83\u662F\u6309\u300C\u5199\u5230\u54EA\u300D\u62E6\u7684\uFF0C\u4E0D\u662F\u6309\u547D\u4EE4\u5934\u4E0A\u90A3\u4E2A\u8BCD\u62E6\u7684\uFF1Ashell \u7684\u5199\u6587\u4EF6\u52A8\u8BCD\u8868\u662F\u62A4\u680F\u4E0D\u662F\u6C99\u7BB1\uFF0C\u6F0F\u4E00\u4E2A\u52A8\u8BCD\uFF0C\u80FD\u81EA\u5DF1\u5199\u6279\u51C6\u56DE\u6267\u7684 agent \u5C31\u7B49\u4E8E\u81EA\u5DF1\u6279\u81EA\u5DF1\uFF08D23/D26\uFF09\u3002\u300C${command.slice(0, 80)}\u300D\u88AB\u62E6\u3002\u8981\u7559\u8BC1\u636E\u7528 run-check / request-approval\uFF1B\u8981\u6539\u56FE\u7684\u53D9\u8FF0\u7528\u7F16\u8F91\u5DE5\u5177\uFF0C\u6539\u72B6\u6001\u7528 set\uFF1B\u770B\u5B83\u7167\u5E38\u653E\u884C \u2014\u2014 cat / rg / gc / sls / git diff / git add ideas/graph.yaml \u90FD\u4E0D\u62E6\u3002`;
}
var INTERPRETER_NAME = String.raw`(python[0-9.]*|py|node|nodejs|deno|bun|ruby|perl|tsx|ts-node|bash|sh|zsh|pwsh|powershell|iex|Invoke-Expression)`;
var LAUNCHER = String.raw`((sudo|npx|bunx|command|env|exec|time|nohup)([ \t]+-\S+)*[ \t]+)*`;
var COMMAND_HEAD = String.raw`(^|[;&|(\r\n\`])[\s;&|(]*`;
var CODE_FLAG = String.raw`-{1,2}(c(ommand)?|e(val)?|enc(odedcommand)?|ec|p(rint)?|f(ile)?)\b`;
var INTERPRETER = new RegExp([
  String.raw`(^|[\s;&|(])${INTERPRETER_NAME}(\.exe)?\s+(-\S+\s+)*${CODE_FLAG}`,
  String.raw`(^|[\s;&|(])${INTERPRETER_NAME}(\.exe)?\s+(-\S+\s+)*\S+\.(py|js|mjs|cjs|ts|mts|cts|tsx|jsx|rb|pl|sh|bash|zsh|ps1|psm1|bat|cmd)\b`,
  String.raw`${COMMAND_HEAD}${LAUNCHER}${INTERPRETER_NAME}(\.exe)?[ \t]+(-\S+[ \t]+)*[^-\s;&|<>]`,
  String.raw`(^|[\s;&|(])${INTERPRETER_NAME}(\.exe)?\s*<`,
  String.raw`\|[ \t]*${LAUNCHER}${INTERPRETER_NAME}(\.exe)?\b`
].join("|"), "i");
var COMPANION_SUBCOMMANDS = SUBCOMMANDS.map(([name]) => name);
var HELP_FLAGS = ["help", "--help", "-h", "--version", "-v", "-V"];
var scriptPath = (file) => String.raw`("(?:[^"\r\n]*[\\/])?${file}"|'(?:[^'\r\n]*[\\/])?${file}'|(?:\S*[\\/])?${file})`;
var ENGINE_PATHS = [
  ENGINE_RELATIVE,
  // .companion/companion.mjs — what install.ts places
  "companion/dist/companion.mjs",
  // the same bundle, freshly built, in this checkout
  "companion/ideas.ts",
  "companion/cli.ts",
  // The pre-unification engines this repository still ships, added on exactly
  // the terms the installers below were: `claude-companion/ideas.ts` is the
  // command this repository's own CLAUDE.md documents AND the engine this
  // checkout actually runs until the migration lands, so refusing it as
  // right-name-wrong-place told the human the documented command was a decoy
  // (D26/D28/D34). They are ordinary project files at fixed paths, judged by
  // identity like every other entry — a same-named file anywhere else is still
  // not an engine.
  //
  // Codex's engine is deliberately NOT here: it is `codex-companion/scripts/
  // companion.py`, a Python script COMPANION_CLI launches no runtime for, with
  // a subcommand table of its own whose `hook` entry is the very door D26 keeps
  // shut. The cost is stated rather than hidden — `python codex-companion/
  // scripts/companion.py validate` stays refused — because widening the
  // allowlist to a hook entry is how an agent writes its own approval receipt.
  "claude-companion/ideas.ts",
  "cursor-companion/ideas.ts"
];
var INSTALL_PATHS = [
  "companion/install.ts",
  "companion/build.mjs",
  "claude-companion/install.ts",
  "cursor-companion/install.ts"
];
function atSanctionedPath(projectDir, token, sanctioned) {
  const bare = token.replace(/^["']|["']$/g, "");
  const rel = relTo(projectDir, bare);
  return sanctioned.some((file) => sameFile2(rel, file));
}
var misplacedScript = (token, sanctioned) => `\u300C${token}\u300D\u6587\u4EF6\u540D\u5BF9\uFF0C\u4F4D\u7F6E\u4E0D\u5BF9 \u2014\u2014 \u8FD9\u51E0\u4E2A\u5165\u53E3\u6309\u8EAB\u4EFD\u8BA4\uFF0C\u4E0D\u6309\u540D\u5B57\u8BA4\uFF08D26/D28\uFF09\u3002\u7B97\u6570\u7684\u53EA\u6709\u9879\u76EE\u91CC\u7684\u8FD9\u51E0\u4E2A\u8DEF\u5F84\uFF1A${sanctioned.join("\u3001")}\u3002\u540D\u5B57\u5BF9\u5C31\u653E\u884C\uFF0C\u7B49\u4E8E\u628A\u767D\u540D\u5355\u501F\u7ED9\u78C1\u76D8\u4E0A\u4EFB\u4F55\u4E00\u4E2A\u540C\u540D\u6587\u4EF6\uFF1A\u5F80\u53EF\u5199\u7684\u76EE\u5F55\uFF08\u6BD4\u5982\u8D26\u672C\u76EE\u5F55 ideas/\uFF09\u91CC\u4E22\u4E00\u4E2A\u540C\u540D\u811A\u672C\u518D\u8DD1\u5B83\uFF0C\u5C31\u662F\u4EFB\u610F\u4EE3\u7801\u6267\u884C\u3002\u771F\u8981\u8DD1\u5F15\u64CE\uFF0C\u5199\u9879\u76EE\u91CC\u90A3\u4E00\u4EFD\u7684\u8DEF\u5F84\uFF1B\u8FD9\u4E2A\u6587\u4EF6\u662F\u666E\u901A\u811A\u672C\uFF0C\u8981\u8DD1\u5B83\u8D70 run-check\uFF08D21\uFF09\u3002`;
var COMPANION_CLI = new RegExp(
  String.raw`^(npx\s+tsx|node|tsx)\s+${scriptPath(String.raw`(?:companion\.mjs|ideas\.ts|cli\.ts)`)}(?:\s+(?:${[...COMPANION_SUBCOMMANDS, ...HELP_FLAGS].join("|")})(?:\s[^;&|<>\r\n]*)?)?\s*$`,
  "i"
);
var SMUGGLED_TAIL = /[\r\n]|\$\(|`/;
var ENGINE_FILE = String.raw`(companion\.mjs|companion\.js|companion\.py|companion[\\/](ideas|guard|cli)\.ts)`;
var ENGINE_MENTION = new RegExp(ENGINE_FILE, "i");
var ENGINE_INVOCATION = new RegExp(
  String.raw`^[\s(]*${LAUNCHER}(${INTERPRETER_NAME}(\.exe)?[ \t]+(-\S+[ \t]+)*)?["']?(\S*[\\/])?${ENGINE_FILE}`,
  "i"
);
var ENGINE_WRITE = new RegExp([
  String.raw`^${LAUNCHER}find\b[^\n]*[ \t]-(delete|exec|execdir|ok|okdir)\b`,
  String.raw`^${LAUNCHER}(truncate|dd|shred|patch|Clear-Content)(\.exe)?\b`
].join("|"), "i");
var COMMAND_SEPARATOR = /[;&|\r\n]+/;
function engineScreen(command) {
  if (!ENGINE_MENTION.test(command)) return null;
  const substitution = /\$\(|`/.test(command);
  const parts = substitution ? [command] : command.split(COMMAND_SEPARATOR).map((part) => part.trim());
  for (const part of parts) {
    if (!ENGINE_MENTION.test(part)) continue;
    if (ENGINE_WRITE.test(part)) {
      return `\u8FD9\u4E00\u6BB5\u662F\u5728\u6539\u5F15\u64CE\u81EA\u5DF1\u7684\u6587\u4EF6\uFF0C\u4E0D\u662F\u5728\u770B\u5B83\uFF08D21/D26\uFF09\uFF1A\u300C${part.slice(0, 80)}\u300D\u3002\u5F15\u64CE\u548C\u5B88\u536B\u4E5F\u662F\u4EA7\u54C1\u4EE3\u7801\uFF0C\u8981\u6539\u5C31\u7528\u7F16\u8F91\u5DE5\u5177\u5199\uFF0C\u8BA9\u5B88\u536B\u6309\u60F3\u6CD5\u56FE\u5224\u4E00\u6B21\uFF1B\u53EA\u662F\u60F3\u770B\u5B83\uFF0C\u7528\u4EC0\u4E48\u529E\u6CD5\u90FD\u884C \u2014\u2014 cat / git diff / gc / sls / awk \u90FD\u4E0D\u62E6\u3002`;
    }
    if (ENGINE_INVOCATION.test(part) || substitution) {
      return `\u5F15\u64CE\u53EA\u80FD\u8FD9\u6837\u8C03\uFF1Anode/tsx/npx tsx <\u8DEF\u5F84> [<\u5B50\u547D\u4EE4>]\uFF0C\u5B50\u547D\u4EE4\u9650 ${COMPANION_SUBCOMMANDS.join(" ")}\uFF08\u53E6\u52A0 ${HELP_FLAGS.join(" / ")}\uFF1B\u4E00\u4E2A\u90FD\u4E0D\u5199\u5C31\u662F\u6253\u5370\u7528\u6CD5\uFF09\u3002\u540E\u9762\u53EF\u4EE5\u63A5\u4E00\u6BB5\u53EA\u8BFB\u7684\u7BA1\u9053\uFF08| head\u3001| less\u3001| Select-Object \u2026\uFF09\uFF0C\u4F46\u4E0D\u8BB8\u63A5\u7B2C\u4E8C\u6761\u547D\u4EE4\u3001\u91CD\u5B9A\u5411\u3001\u6362\u884C\u7EED\u884C\u6216\u547D\u4EE4\u66FF\u6362 \u2014\u2014 \u90A3\u4E9B\u62C6\u6210\u4E24\u6B21\u8C03\u7528\uFF08D28\uFF09\u3002\u624B\u5DE5\u8DD1 guard/hook \u5165\u53E3\u7B49\u4E8E\u81EA\u5DF1\u9020\u4E8B\u4EF6\u3001\u7ED9\u81EA\u5DF1\u7B7E\u6279\u51C6\uFF0C\u6C38\u8FDC\u4E0D\u653E\u884C\uFF08D26\uFF09\uFF1A\u300C${command.slice(0, 80)}\u300D`;
    }
  }
  return null;
}
var PIPED_RUNTIME = new RegExp(String.raw`^${LAUNCHER}${INTERPRETER_NAME}(\.exe)?\b`, "i");
var inertStage = (stage, projectDir) => stage !== "" && !ENGINE_MENTION.test(stage) && mutatingShell(stage) === null && !INTERPRETER.test(stage) && !PIPED_RUNTIME.test(stage) && protectedTargetRefusal(stage, projectDir) === null;
var SANCTIONED_SCRIPT = new RegExp(
  String.raw`^(npx\s+tsx|node|tsx)\s+${scriptPath(String.raw`(?:companion[\\/](?:install\.ts|build\.mjs)|(?:claude|cursor)-companion[\\/]install\.ts)`)}(?:\s[^;&|<>\r\n]*)?$`,
  "i"
);
function sanctionedScript(command, projectDir) {
  const hit = SANCTIONED_SCRIPT.exec(command);
  return hit !== null && atSanctionedPath(projectDir, hit[2], INSTALL_PATHS);
}
function ruleShell(event, projectDir) {
  const command = (event.command ?? "").trim();
  if (!command) return OK;
  let declared;
  try {
    const graph = loadGraphStrict(projectDir);
    declared = graph.ideas.find((i) => i.status === "doing" && i.verify?.command?.trim() === command);
    const chained = declared && chainedCommandRefusal(declared, command);
    if (chained) return { allow: false, reason: chained };
    if (declared && validApproval(projectDir, graph, "plan", [declared.id])) return OK;
  } catch {
  }
  const verdict = screenShell(command, projectDir);
  if (verdict.allow || !declared) return verdict;
  return {
    allow: false,
    reason: `\u8FD9\u662F ${declared.id}\u300C${declared.name}\u300D\u5728\u56FE\u91CC\u58F0\u660E\u7684 verify.command\uFF08\u300C${command.slice(0, 80)}\u300D\uFF09\uFF0C\u62E6\u4E0B\u5B83\u7684\u4E0D\u662F\u547D\u4EE4\u672C\u8EAB\uFF0C\u662F\u8FD9\u4E2A\u60F3\u6CD5\u7684\u65B9\u6848\u8FD8\u6CA1\u6709\u5F53\u524D\u6709\u6548\u7684\u4EBA\u5DE5\u6279\u51C6\uFF08D7\uFF09\uFF1A\u53BB\u8981\u4E00\u6B21 \u2014\u2014 request-approval --gate plan --node ${declared.id}\uFF0C\u4EBA\u56DE\u300C\u6279\u51C6\u300D\u4E4B\u540E\u8FD9\u6761\u547D\u4EE4\u5C31\u7167\u539F\u6837\u653E\u884C\u3002\uFF08\u6279\u51C6\u7ED1\u5728\u56FE\u7684\u5185\u5BB9\u4E0A\uFF1A\u4E4B\u540E\u518D\u6539 how / verify\uFF0C\u6279\u51C6\u4F5C\u5E9F\uFF0C\u8981\u91CD\u65B0\u8981\u3002\u82E5\u786E\u5B9E\u60F3\u6539\u547D\u4EE4\u672C\u8EAB\uFF0C\u5148\u6539\u56FE\u518D\u8981\u6279\u51C6\u3002\u539F\u672C\u6321\u4F4F\u5B83\u7684\u89C4\u5219\uFF1A${verdict.reason}\uFF09`
  };
}
function screenShell(command, projectDir) {
  if (!SMUGGLED_TAIL.test(command)) {
    const [head, ...downstream] = command.split("|").map((stage) => stage.trim());
    if (downstream.every((stage) => inertStage(stage, projectDir))) {
      const engine2 = COMPANION_CLI.exec(head);
      if (engine2) {
        return atSanctionedPath(projectDir, engine2[2], ENGINE_PATHS) ? OK : { allow: false, reason: misplacedScript(engine2[2], ENGINE_PATHS) };
      }
      const script = SANCTIONED_SCRIPT.exec(head);
      if (script && mutatingShell(head) === null) {
        return atSanctionedPath(projectDir, script[2], INSTALL_PATHS) ? OK : { allow: false, reason: misplacedScript(script[2], INSTALL_PATHS) };
      }
    }
  }
  const engine = engineScreen(command);
  if (engine) return { allow: false, reason: engine };
  const protectedFile = protectedTargetRefusal(command, projectDir);
  if (protectedFile) return { allow: false, reason: protectedFile };
  const mutation = mutatingShell(command);
  if (mutation !== null) return { allow: false, reason: mutatingReason(command, mutation) };
  if (INTERPRETER.test(command) && !(sanctionedScript(command, projectDir) && !SMUGGLED_TAIL.test(command))) {
    return { allow: false, reason: `\u89E3\u91CA\u5668\uFF08python -c / node --eval / powershell -enc <base64> / python \u811A\u672C.py / bash \u6CA1\u6709\u6269\u5C55\u540D\u7684\u811A\u672C / deno run \u811A\u672C / \u7BA1\u9053\u53F3\u8FB9\u7684 \u2026 | bash \u2026\uFF09\u7ED5\u5F97\u8FC7\u8DEF\u5F84\u68C0\u67E5\uFF0C\u7EDF\u4E00\u8D70 run-check\uFF08D21\uFF09\u3002\u672C\u4ED3\u5E93\u81EA\u5DF1\u7684\u5B89\u88C5\u5668\u548C\u6253\u5305\u5165\u53E3\u9664\u5916\uFF1A${INSTALL_PATHS.join("\u3001")}\uFF08\u8DEF\u5F84\u91CC\u6709\u7A7A\u683C\u5C31\u7ED9\u5B83\u52A0\u4E00\u5BF9\u5F15\u53F7\uFF1B\u5FC5\u987B\u662F\u9879\u76EE\u91CC\u7684\u90A3\u4E00\u4EFD\uFF0C\u540C\u540D\u7684\u522B\u5904\u6587\u4EF6\u4E0D\u7B97\uFF09\u3002` };
  }
  return OK;
}
function ruleStop(event, projectDir) {
  if (event.stop_hook_active) return OK;
  if (!existsSync2(graphPath(projectDir))) return OK;
  const graph = load(graphPath(projectDir)).graph;
  const { errors } = check(graph, projectDir);
  if (errors.length === 0) return OK;
  return {
    allow: false,
    reason: `\u60F3\u6CD5\u56FE\u6709 ${errors.length} \u4E2A\u9519\u8BEF\uFF0C\u4FEE\u5B8C\u518D\u7ED3\u675F\uFF08R5\uFF09\uFF1A
${errors.map((e) => `  - ${e}`).join("\n")}`
  };
}
function record(event, projectDir) {
  try {
    const targets = [...event.paths ?? [], ...(event.operations ?? []).map((op) => op.path)];
    if (targets.length === 0) return OK;
    let graph = null;
    try {
      graph = load(graphPath(projectDir)).graph;
    } catch {
      graph = null;
    }
    for (const target of targets) {
      const rel = relTo(projectDir, target);
      if (graph) {
        try {
          recordChange(projectDir, graph, rel);
        } catch {
        }
      }
      try {
        const file = paths(projectDir).log;
        mkdirSync2(dirname2(file), { recursive: true });
        const stamp = (/* @__PURE__ */ new Date()).toISOString().replace("T", " ").slice(0, 16);
        appendFileSync2(file, `- ${stamp}  ${event.tool ?? "write"} ${rel}
`);
      } catch {
      }
    }
    return OK;
  } catch (error) {
    return { allow: true, warn: `\u5199\u540E\u8BB0\u5F55\u5931\u8D25\uFF08${error instanceof Error ? error.message : error}\uFF09\u2014\u2014 \u653E\u884C\u4F46\u6CA1\u8BB0\u4E0A\uFF08D9\uFF09\u3002` };
  }
}
var CLAUDE_WRITE_TOOLS = /^(Edit|Write|NotebookEdit)$/;
var CURSOR_WRITE_TOOLS2 = /^(Write|StrReplace|Delete|EditNotebook|ApplyPatch|search_replace)$/i;
var SHELL_TOOLS2 = /* @__PURE__ */ new Set(["bash", "powershell", "pwsh", "shell", "local_shell"]);
var isShellTool = (tool) => SHELL_TOOLS2.has(tool.toLowerCase());
var READ_TOOLS2 = /* @__PURE__ */ new Set(["read", "read_file", "readfile"]);
var isReadTool = (tool) => READ_TOOLS2.has(tool.toLowerCase());
var MCP_WRITEISH = /write|edit|create|delete|update|move|save|patch|append|remove|rename/i;
function asRecord(value) {
  if (typeof value === "string") {
    try {
      return JSON.parse(value);
    } catch {
      return {};
    }
  }
  return value && typeof value === "object" ? value : {};
}
function extractPath(input) {
  for (const key of ["file_path", "path", "filePath", "uri", "target_notebook", "notebook_path"]) {
    const value = input[key];
    if (typeof value === "string" && value) return value;
  }
  return void 0;
}
function extractCommand(input) {
  for (const key of ["command", "script"]) {
    const value = input[key];
    if (typeof value === "string" && value) return value;
    if (Array.isArray(value) && value.length > 0) return value.map(String).join(" ");
  }
  return void 0;
}
function editOf(input) {
  return {
    old_string: input["old_string"],
    new_string: input["new_string"],
    content: input["content"],
    replace_all: input["replace_all"]
  };
}
function patchTextOf(input) {
  for (const key of ["patch", "diff", "patch_text"]) {
    const value = input[key];
    if (typeof value === "string" && value) return value;
  }
  return void 0;
}
var MCP_PATH_KEY = /(^|[_.-])(path|paths|file|files|filename|filepath|dest|destination|target|targets|location|notebook|dir|directory|folder|out|output)([_.-]|$)/i;
var MCP_PATH_VALUE = /^(?!\w+:\/\/)(?:\S*[\\/]\S*|[^\s\\/]+\.[A-Za-z0-9]{1,8}|.*[\\/][^\\/\s]*\.[A-Za-z0-9]{1,8})$/;
var MCP_CONTENT_KEY = /^(content|contents|file_text|new_string|new_content|patch|diff)$/i;
function mcpTargets(input) {
  const known = extractPath(input);
  if (known) return [known];
  const found = [];
  for (const [key, value] of Object.entries(input)) {
    if (!MCP_PATH_KEY.test(key)) continue;
    for (const item of Array.isArray(value) ? value : [value]) {
      if (typeof item === "string" && item && MCP_PATH_VALUE.test(item)) found.push(item);
    }
  }
  return found;
}
function mcpEvent(kind, tool, input, raw) {
  const targets = mcpTargets(input);
  if (targets.length > 0) return { event: kind, tool, paths: targets, cwd: raw.cwd };
  if (MCP_WRITEISH.test(tool) || Object.keys(input).some((key) => MCP_CONTENT_KEY.test(key))) {
    return { event: kind, tool, paths: [], unknownTarget: kind === "pre-write", cwd: raw.cwd };
  }
  return { event: "other", tool, cwd: raw.cwd };
}
var PATCH_HEADER = /^([ \t]*)\*{3} (.+?)\s*$/gm;
var PATCH_OP = /^(Add|Update|Delete) File: (.+)$/;
var PATCH_MOVE = /^Move to: (.+)$/;
var PATCH_FRAME = /^(Begin Patch|End Patch|End of Patch|End of File)$/;
function patchOperations(text) {
  const operations = [];
  let unknownHeader = false;
  for (const m of text.matchAll(PATCH_HEADER)) {
    const header = m[2];
    if (m[1] !== "") {
      unknownHeader = true;
      continue;
    }
    const op = PATCH_OP.exec(header);
    if (op) {
      operations.push({ kind: op[1].toLowerCase(), path: op[2] });
      continue;
    }
    const move = PATCH_MOVE.exec(header);
    if (move) {
      operations.push({ kind: "add", path: move[1] });
      continue;
    }
    if (!PATCH_FRAME.test(header)) unknownHeader = true;
  }
  return { operations, unknownHeader };
}
function normalizeClaude(raw) {
  const input = asRecord(raw.tool_input);
  const tool = raw.tool_name ?? "";
  switch (raw.hook_event_name) {
    case "PreToolUse":
      if (isShellTool(tool)) return { event: "shell", tool, command: extractCommand(input), cwd: raw.cwd };
      if (CLAUDE_WRITE_TOOLS.test(tool)) {
        const path = extractPath(input);
        return { event: "pre-write", tool, paths: path ? [path] : [], unknownTarget: !path, edit: editOf(input), cwd: raw.cwd };
      }
      if (tool.startsWith("mcp__")) return mcpEvent("pre-write", tool, input, raw);
      return { event: "other", tool, cwd: raw.cwd };
    case "PostToolUse": {
      const path = extractPath(input);
      if (!path) return { event: "other", tool, paths: [], cwd: raw.cwd };
      if (isReadTool(tool)) return { event: "read", tool, paths: [path], cwd: raw.cwd };
      return { event: "post-write", tool, paths: [path], cwd: raw.cwd };
    }
    case "UserPromptSubmit":
      return { event: "prompt", prompt: raw.prompt, cwd: raw.cwd, session_id: raw.session_id, turn_id: raw.turn_id };
    case "SessionStart":
      return { event: "session", cwd: raw.cwd };
    case "Stop":
      return { event: "stop", stop_hook_active: raw.stop_hook_active === true, cwd: raw.cwd };
    default:
      return { event: "other", cwd: raw.cwd };
  }
}
function encodeClaude(event, verdict) {
  if (verdict.allow) {
    return { exitCode: 0, stdout: verdict.message ? verdict.message + "\n" : void 0, stderr: verdict.warn };
  }
  if (event.event === "pre-write" || event.event === "shell") {
    return {
      exitCode: 2,
      stdout: JSON.stringify({ hookSpecificOutput: {
        hookEventName: "PreToolUse",
        permissionDecision: "deny",
        permissionDecisionReason: verdict.reason ?? "blocked"
      } }),
      stderr: verdict.reason
    };
  }
  return { exitCode: 2, stderr: verdict.reason };
}
function normalizeCursor(raw) {
  const input = asRecord(raw.tool_input);
  const tool = raw.tool_name ?? "";
  switch (raw.hook_event_name) {
    case "preToolUse":
      if (CURSOR_WRITE_TOOLS2.test(tool)) {
        const path = extractPath(input);
        return { event: "pre-write", tool, paths: path ? [path] : [], unknownTarget: !path, edit: editOf(input), patchText: patchTextOf(input), cwd: raw.cwd };
      }
      if (tool.startsWith("mcp__") || tool.startsWith("MCP:")) return mcpEvent("pre-write", tool, input, raw);
      return { event: "other", tool, cwd: raw.cwd };
    case "beforeShellExecution":
      return { event: "shell", command: raw.command, cwd: raw.cwd };
    case "beforeMCPExecution":
      return mcpEvent("pre-write", tool, input, raw);
    case "afterFileEdit": {
      const path = raw.file_path ?? extractPath(input);
      return { event: "post-write", tool: "Edit", paths: path ? [path] : [], cwd: raw.cwd };
    }
    case "beforeReadFile": {
      const path = raw.file_path ?? extractPath(input);
      return path ? { event: "read", tool: tool || "Read", paths: [path], cwd: raw.cwd } : { event: "other", tool, cwd: raw.cwd };
    }
    case "beforeSubmitPrompt":
      return { event: "prompt", prompt: raw.prompt, cwd: raw.cwd, session_id: raw.session_id, turn_id: raw.turn_id };
    case "sessionStart":
      return { event: "session", cwd: raw.cwd };
    case "stop":
      return { event: "stop", stop_hook_active: (raw.loop_count ?? 0) > 0, cwd: raw.cwd };
    default:
      return { event: "other", cwd: raw.cwd };
  }
}
function encodeCursor(event, verdict) {
  if (event.event === "stop") {
    return { exitCode: 0, stdout: JSON.stringify(verdict.allow ? {} : { followup_message: verdict.reason }) };
  }
  if (event.event === "prompt") {
    return {
      exitCode: 0,
      stdout: JSON.stringify({ continue: true, ...verdict.message ? { agent_message: verdict.message } : {} })
    };
  }
  if (event.event === "session") {
    return { exitCode: 0, stdout: JSON.stringify(verdict.message ? { additional_context: verdict.message } : {}) };
  }
  if (event.event === "post-write" || event.event === "read" || event.event === "other") {
    return { exitCode: 0, stdout: "{}" };
  }
  return {
    exitCode: 0,
    // Cursor reads the JSON, not the exit code
    stdout: JSON.stringify(verdict.allow ? { permission: "allow" } : { permission: "deny", user_message: verdict.reason, agent_message: `Companion \u62E6\u4E0B\u4E86\u8FD9\u6B21\u64CD\u4F5C\u3002${verdict.reason ?? ""}` })
  };
}
function normalizeCodex(raw) {
  const input = asRecord(raw.tool_input);
  const tool = raw.tool_name ?? "";
  if (raw.hook_event_name === "PreToolUse" || raw.hook_event_name === "PostToolUse") {
    const kind = raw.hook_event_name === "PreToolUse" ? "pre-write" : "post-write";
    if (tool === "apply_patch") {
      const text = String(input["command"] ?? input["patch"] ?? "");
      const { operations, unknownHeader } = patchOperations(text);
      if (operations.length === 0 || unknownHeader) {
        return { event: kind, tool, paths: [], unknownTarget: kind === "pre-write", cwd: raw.cwd };
      }
      return { event: kind, tool, operations, paths: [], patchText: text, cwd: raw.cwd };
    }
    if (isShellTool(tool)) {
      return kind === "pre-write" ? { event: "shell", tool, command: extractCommand(input), cwd: raw.cwd } : { event: "other", tool, cwd: raw.cwd };
    }
    if (isReadTool(tool)) {
      const path = extractPath(input);
      return kind === "post-write" && path ? { event: "read", tool, paths: [path], cwd: raw.cwd } : { event: "other", tool, cwd: raw.cwd };
    }
    if (/^(Edit|Write)$/.test(tool)) {
      const path = extractPath(input);
      return kind === "pre-write" ? { event: kind, tool, paths: path ? [path] : [], unknownTarget: !path, edit: editOf(input), cwd: raw.cwd } : { event: kind, tool, paths: path ? [path] : [], cwd: raw.cwd };
    }
    if (tool.startsWith("mcp__")) return mcpEvent(kind, tool, input, raw);
    return { event: "other", tool, cwd: raw.cwd };
  }
  if (raw.hook_event_name === "UserPromptSubmit") {
    return { event: "prompt", prompt: raw.prompt, cwd: raw.cwd, session_id: raw.session_id, turn_id: raw.turn_id };
  }
  if (raw.hook_event_name === "SessionStart") {
    return { event: "session", cwd: raw.cwd };
  }
  if (raw.hook_event_name === "Stop") {
    return { event: "stop", stop_hook_active: raw.stop_hook_active === true, cwd: raw.cwd };
  }
  return { event: "other", cwd: raw.cwd };
}
function encodeCodex(event, verdict) {
  if (verdict.allow) {
    return { exitCode: 0, stdout: verdict.message ? verdict.message + "\n" : void 0, stderr: verdict.warn };
  }
  if (event.event === "stop") {
    return { exitCode: 0, stdout: JSON.stringify({ decision: "block", reason: verdict.reason ?? "blocked" }) };
  }
  return {
    exitCode: 2,
    stdout: JSON.stringify({ hookSpecificOutput: {
      hookEventName: "PreToolUse",
      permissionDecision: "deny",
      permissionDecisionReason: verdict.reason ?? "blocked"
    } }),
    stderr: verdict.reason
  };
}
function handlePrompt(event, projectDir) {
  if (event.event !== "prompt" || !event.prompt) return null;
  return applyApproval(projectDir, event.prompt, {
    date: (/* @__PURE__ */ new Date()).toISOString().slice(0, 10),
    session_id: event.session_id,
    turn_id: event.turn_id
  });
}
function sessionBriefing(projectDir) {
  const lines = [];
  const real = console.log;
  console.log = (...parts) => {
    lines.push(parts.map(String).join(" "));
  };
  try {
    main(["status", "--project", projectDir]);
  } catch {
    return void 0;
  } finally {
    console.log = real;
  }
  return lines.length > 0 ? `Companion \u60F3\u6CD5\u56FE\u73B0\u72B6\uFF08status\uFF09\uFF1A
${lines.join("\n")}` : void 0;
}
var ENCODERS = { claude: encodeClaude, cursor: encodeCursor, codex: encodeCodex };
var NORMALIZERS = { claude: normalizeClaude, cursor: normalizeCursor, codex: normalizeCodex };
function resolvePlatform(args2) {
  const requested = args2.find((a) => a.startsWith("--platform="))?.slice(11) ?? "claude";
  return Object.hasOwn(ENCODERS, requested) ? requested : null;
}
async function readStdin() {
  const chunks = [];
  for await (const chunk of process.stdin) chunks.push(chunk);
  return Buffer.concat(chunks).toString("utf8");
}
function runGuard(args2) {
  const platformArg = resolvePlatform(args2);
  if (platformArg === null) {
    process.stderr.write(`Companion \u5B88\u536B\uFF1A\u4E0D\u8BA4\u8BC6 --platform=${args2.find((a) => a.startsWith("--platform="))?.slice(11) ?? ""}\uFF0C\u8BA4\u5F97\u7684\u662F ${Object.keys(ENCODERS).join(" / ")}\u3002\u63A5\u7EBF\u91CC\u62FC\u9519\u4E00\u4E2A\u5B57\u6BCD\u5C31\u7B49\u4E8E\u5B88\u536B\u6CA1\u63A5\u4E0A\uFF0C\u6240\u4EE5\u8FD9\u91CC\u76F4\u63A5\u62A5\u9519\u9000\u51FA\uFF0C\u4E0D\u6084\u6084\u6309 Claude \u7684\u8BED\u4E49\u56DE\u7B54\uFF08D9/D15\uFF09\u3002
`);
    process.exit(2);
    return;
  }
  readStdin().then((rawText) => {
    let raw;
    try {
      raw = JSON.parse(rawText || "{}");
    } catch {
      process.exit(0);
      return;
    }
    const projectDir = projectRoot(raw.cwd ?? process.cwd());
    const guardOff = process.env.AIDEV_GUARD === "off";
    const normalize = NORMALIZERS[platformArg];
    const encode = ENCODERS[platformArg];
    const event = normalize(raw);
    if (guardOff) {
      try {
        mkdirSync2(dirname2(paths(projectDir).log), { recursive: true });
        appendFileSync2(
          paths(projectDir).log,
          `- ${(/* @__PURE__ */ new Date()).toISOString().replace("T", " ").slice(0, 16)}  guard.disabled  AIDEV_GUARD=off \u671F\u95F4\u53D1\u751F ${event.event}
`
        );
      } catch {
      }
    }
    let message;
    if (event.event === "prompt" && !guardOff) {
      const outcome = handlePrompt(event, projectDir);
      if (outcome) {
        message = outcome.ok ? `Companion\uFF1A${outcome.decision === "approved" ? "\u6279\u51C6" : "\u62D2\u7EDD"}\u5DF2\u8BB0\u5F55\uFF08${outcome.gate}\uFF09\u3002` : `Companion\uFF1A${outcome.reason}`;
      }
    }
    if (event.event === "session") message = sessionBriefing(projectDir);
    if (event.event === "read") {
      try {
        for (const path of event.paths ?? []) strike(projectDir, path);
      } catch {
      }
    }
    if (event.event === "post-write" && !guardOff) {
      const recorded = record(event, projectDir);
      if (recorded.warn) process.stderr.write(recorded.warn + "\n");
      process.exit(0);
      return;
    }
    const verdict = { ...decide(event, projectDir, { guardOff }), ...message ? { message } : {} };
    const reply = encode(event, verdict);
    if (verdict.warn) process.stderr.write(verdict.warn + "\n");
    if (reply.stdout) process.stdout.write(reply.stdout);
    if (reply.stderr && !verdict.warn) process.stderr.write(reply.stderr);
    process.exit(reply.exitCode);
  });
}
if (process.argv[1]?.endsWith("guard.ts")) runGuard(process.argv.slice(2));

// companion/cli.ts
var args = process.argv.slice(2);
if (args[0] === "guard") {
  runGuard(args.slice(1));
} else {
  try {
    const code = main(args);
    if (code !== -1) process.exit(code);
  } catch (error) {
    console.error(String(error instanceof Error ? error.message : error));
    process.exit(1);
  }
}
