#include "jsapi.hpp"

// // There's code in SpiderMonkey that unconditionally calls getpid() instead of the MOZ_GET_PID() macro.
// extern "C" int getpid() {
//   return -1;
// }

namespace jsglue {
extern "C" {
const JSClass* GetClass(const JSObject* obj) {
  return JS::GetClass(obj);
}
  // This creates a JSFunction and sets its length and name properties in the
// order that ECMAScript's CreateBuiltinFunction does.
static JSObject* CreateBuiltinFunctionForConstructor(
    JSContext* cx, JSNative jsCtor, unsigned int argc,
    const JSClass* cls, JS::HandleId name, JS::HandleObject proto) {

  JSFunction* fun = js::NewFunctionByIdWithReservedAndProto(
      cx, jsCtor, proto, argc, JSFUN_CONSTRUCTOR, name);
  if (!fun) {
    return nullptr;
  }

  JS::RootedObject constructor(cx, JS_GetFunctionObject(fun));
  js::SetFunctionNativeReserved(constructor, 0, JS::PrivateValue((void*)cls));

  // Eagerly force creation of the .length and .name properties, because
  // SpiderMonkey creates them lazily (see
  // https://bugzilla.mozilla.org/show_bug.cgi?id=1629803).
  bool unused;
  if (!JS_HasProperty(cx, constructor, "length", &unused) ||
      !JS_HasProperty(cx, constructor, "name", &unused)) {
    return nullptr;
      }

  return constructor;
}

static bool DefineConstructor(JSContext* cx, JS::HandleObject global, JS::HandleId name,
                              JS::HandleObject constructor) {
  bool alreadyDefined;
  if (!JS_AlreadyHasOwnPropertyById(cx, global, name, &alreadyDefined)) {
    return false;
  }

  // This is Enumerable: False per spec.
  return alreadyDefined ||
         JS_DefinePropertyById(cx, global, name, constructor, JSPROP_RESOLVING);
}

struct ConstantSpec {
  const char* name;
  JS::Value value;
};

struct NativeProperties {
  const JSFunctionSpec* methods;
  const JSPropertySpec* properties;
  const ConstantSpec* constants;
};

bool DefineConstants(JSContext* cx, JS::HandleObject obj, const ConstantSpec* cs) {
  JS::Rooted<JS::Value> value(cx);
  for (; cs->name; ++cs) {
    value = cs->value;
    bool ok = JS_DefineProperty(
        cx, obj, cs->name, value,
        JSPROP_ENUMERATE | JSPROP_READONLY | JSPROP_PERMANENT);
    if (!ok) {
      return false;
    }
  }
  return true;
}

bool InitProperties(
    JSContext* cx, JS::HandleObject obj,
    const NativeProperties* properties) {
  if (properties) {
    if (properties->methods && !JS_DefineFunctions(cx, obj, properties->methods)) {
      return false;
    }

    if (properties->properties && !JS_DefineProperties(cx, obj, properties->properties)) {
      return false;
    }

    if (properties->constants && !DefineConstants(cx, obj, properties->constants)) {
      return false;
    }
  }

  return true;
}

bool DefineToStringTag(JSContext* cx, JS::Handle<JSObject*> obj,
                              JS::Handle<JSString*> class_name) {
  JS::Rooted<jsid> toStringTagId(
      cx, JS::GetWellKnownSymbolKey(cx, JS::SymbolCode::toStringTag));
  return JS_DefinePropertyById(cx, obj, toStringTagId, class_name,
                               JSPROP_READONLY);
}

// name must be an atom (or JS::PropertyKey::NonIntAtom will assert).
JSObject* CreateBuiltinClass(
    JSContext* cx, JSNative jsCtor, unsigned argc,
    const JSClass* cls, const NativeProperties* properties,
    const NativeProperties* ctorProperties,
    const JSClass* protoClass, JS::HandleObject protoProto,
    JS::HandleObject global, bool defineOnGlobal) {

  MOZ_ASSERT(cls);

  JS::RootedString nameStr(cx, JS_AtomizeString(cx, cls->name));
  if (!nameStr) {
    return nullptr;
  }
  JS::RootedId nameId(cx, JS::PropertyKey::NonIntAtom(nameStr));

  JS::RootedObject proto(cx);
  if (protoClass) {
    proto = JS_NewObjectWithGivenProto(cx, protoClass, protoProto);
    if (!proto) {
      return nullptr;
    }
  }

  if (!InitProperties(cx, proto, properties)) {
    return nullptr;
  }

  if (!DefineToStringTag(cx, proto, nameStr)) {
    return nullptr;
  }


  JS::RootedObject constructor(cx);
  constructor = CreateBuiltinFunctionForConstructor(cx, jsCtor, argc, cls, nameId, proto);
  if (!constructor) {
    return nullptr;
  }

  if (proto && !JS_LinkConstructorAndPrototype(cx, constructor, proto)) {
    return nullptr;
  }

  if (!InitProperties(cx, constructor, ctorProperties)) {
    return nullptr;
  }

  if (defineOnGlobal && !DefineConstructor(cx, global, nameId, constructor)) {
    return nullptr;
  }

  return constructor;
}

}
}  // namespace jsglue
