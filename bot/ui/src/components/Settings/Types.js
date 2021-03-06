import {String} from "./String";
import {Text} from "./Text";
import {Duration} from "./Duration";
import {Boolean} from "./Boolean";
import {Number} from "./Number";
import {Percentage} from "./Percentage";
import {Raw} from "./Raw";
import {Set} from "./Set";
import {Select} from "./Select";
import {Oauth2Config} from "./Oauth2Config";
import * as Format from "./Format";

/**
 * Decode the given type and value.
 *
 * @param {object} type the type to decode
 * @param {any} value the value to decode
 */
export function decode(type) {
  if (type === null) {
    throw new Error(`bad type: ${type}`);
  }

  let format = null;
  let placeholder = null;
  let value = null;

  switch (type.id) {
    case "oauth2-config":
      return new Oauth2Config(type.optional);
    case "duration":
      return new Duration(type.optional);
    case "bool":
      return new Boolean(type.optional);
    case "string":
      format = Format.decode(type.format);
      placeholder = type.placeholder;
      return new String(type.optional, format, placeholder);
    case "text":
      return new Text(type.optional);
    case "number":
      return new Number(type.optional);
    case "percentage":
      return new Percentage(type.optional);
    case "set":
      value = decode(type.value);
      return new Set(type.optional, value);
    case "select":
      value = decode(type.value);
      return new Select(type.optional, value, type.options);
    default:
      return new Raw(type.optional);
  }
}