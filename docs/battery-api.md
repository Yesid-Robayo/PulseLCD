# Battery source · Fuente de baterías

[English](#english) · [Español](#español)

## English

PulseLCD never talks to your peripherals. To show battery alerts it reads, every 30 seconds, a local address that answers JSON.
[YesCharge](https://github.com/Yesid-Robayo/YesCharge) does this at `http://127.0.0.1:8765/v1/status`, and any program can publish the
same format:

```json
{
  "devices": [
    { "kind": "mouse", "charge": "discharging", "low": true, "full": false },
    { "kind": "headset", "charge": "full", "low": false, "full": true }
  ]
}
```

| Field | Values | Meaning |
| --- | --- | --- |
| `kind` | `headset`, `mouse`, `keyboard`, `gamepad` | Which icon to show. Anything else is ignored. |
| `charge` | `charging`, others | A low device on the cable shows no warning. |
| `low` | `true` / `false` | Shows the amber icon. |
| `full` | `true` / `false` | Shows the green icon. |

Other fields are ignored. The address can only be local (`127.0.0.1` or `localhost`): device names don't leave your PC. If it stops
answering, the panel shows a broken link icon.

## Español

PulseLCD nunca habla con tus periféricos. Para mostrar las alertas de batería lee, cada 30 segundos, una dirección local que responde JSON.
[YesCharge](https://github.com/Yesid-Robayo/YesCharge) lo hace en `http://127.0.0.1:8765/v1/status`, y cualquier programa puede publicar el
mismo formato (el ejemplo de arriba).

| Campo | Valores | Significado |
| --- | --- | --- |
| `kind` | `headset`, `mouse`, `keyboard`, `gamepad` | Qué icono mostrar. Lo demás se ignora. |
| `charge` | `charging`, otros | Un dispositivo bajo en el cable no avisa. |
| `low` | `true` / `false` | Muestra el icono ámbar. |
| `full` | `true` / `false` | Muestra el icono verde. |

Los demás campos se ignoran. La dirección solo puede ser local (`127.0.0.1` o `localhost`): los nombres de tus dispositivos no salen de tu
PC. Si deja de responder, el panel muestra un icono de enlace roto.
