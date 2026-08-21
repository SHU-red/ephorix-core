# EphoriX — Import Adapter

`POST /api/v1/import` is the generic batch-import path for anything that is
**not** the Pebble watch: exported files and offline dumps from Google Health
Connect, Apple Health, Garmin Connect, or hand-assembled CSVs. Adapters (or a
human) flatten each source's records into timestamped samples and POST them;
the backend normalizes each sample against the canonical metric vocabulary
and stores it in the same long-form `measurements` table the watch writes to.

Auth: same as every other route — `X-EphoriX-Token: <token>` header.

---

## 1. Payload contract

### Request

```jsonc
{
  "source": "health_connect",         // REQUIRED: pebble | fitbit | garmin |
                                       //          apple_health | manual |
                                       //          health_connect | csv | gpx
  "deviceId": "pixel-9",              // optional, informational
  "samples": [
    {
      "timestamp": "2026-08-21T07:30:00Z", // REQUIRED, RFC-3339 / ISO-8601
      "metric": "heart_rate",              // REQUIRED, canonical name (below)
      "value": 62.0,                       // REQUIRED, finite number, canonical unit
      "unit": "bpm",                       // optional, recorded as-is
      "meta": { "rawType": "health.dataType:heartRate" } // optional, free-form, NOT persisted
    }
  ]
}
```

### Response `200`

```json
{ "inserted": 118, "skipped": 2, "errors": ["sample 14: unknown metric 'vo2_max'", "sample 201: invalid timestamp '2026-08-21'"] }
```

- **Validation is per-sample, not per-request.** A sample whose timestamp
  does not parse, whose metric is not canonical, or whose value is not
  finite is counted in `skipped` and reported in `errors`; it never aborts
  the batch. Only a bad `source`, an empty `samples` array, or a malformed
  body returns an error status (`{"error": {"code": "bad_request", ...}}`).
- `errors` is capped at the first **20** per-sample messages; `skipped`
  keeps counting beyond that.
- All valid rows land in **one transaction** (same as `/api/v1/ingest`).
- `meta` is accepted for forward compatibility (source-specific coercion)
  but is not stored.
- Idempotency is the caller's problem: re-importing the same export
  duplicates rows (same as every other EphoriX write).

---

## 2. Canonical metric vocabulary

The single vocabulary for the normalized store (`normalize.rs`). Adapters
MUST convert to these names **and units** before POSTing; the endpoint
rejects (skips) anything else.

| Metric                 | Unit | Meaning                                   |
| ---------------------- | ---- | ----------------------------------------- |
| `heart_rate`           | bpm  | Instantaneous / sample heart rate         |
| `resting_hr`           | bpm  | Resting heart rate (overnight minimums)   |
| `hrv`                  | ms   | Heart-rate variability                    |
| `steps`                | count| Step count (total or delta, as reported)  |
| `distance_m`           | m    | Travel distance                           |
| `active_calories`      | kcal | Active / movement calories                |
| `resting_kcal`         | kcal | Resting (basal) calories                  |
| `active_seconds`       | s    | Duration of active time                   |
| `sleep_seconds`        | s    | Total sleep duration                      |
| `restful_sleep_seconds`| s    | Deep / restorative sleep duration         |
| `movement_intensity`   | au   | Arbitrary 0–100 movement-intensity scale  |
| `reps`                 | count| Reps of a workout                         |
| `water_ml`             | ml   | Water intake                              |
| `food_kcal`            | kcal | Food energy intake                        |
| `protein_g`            | g    | Protein intake                            |
| `carbs_g`              | g    | Carbohydrate intake                       |
| `fat_g`                | g    | Fat intake                                |

Not in this vocabulary (no canonical home yet — skip them): body weight,
blood pressure, respiratory rate, SpO2, VO2max, body temperature, fiber,
total (non-active) calories.

---

## 3. Mapping tables

### 3.1 Google Health Connect (export via `hc` CLI)

Each `hc` record has `dataType`, `timeIntervalStart/End`, and per-type value
fields. One HC record → one or more import samples (use the interval start
as `timestamp` for interval types).

| Health Connect data type          | Value field      | → Canonical metric   | Unit |
| --------------------------------- | ---------------- | -------------------- | ---- |
| `health.dataType:heartRate`       | `heartRate`      | `heart_rate`         | bpm  |
| `health.dataType:restingHeartRate`| `restingHeartRate` | `resting_hr`       | bpm  |
| `health.dataType:stepCount`       | `stepCount`      | `steps`              | count|
| `health.dataType:distance`        | `distance`       | `distance_m`         | m    |
| `health.dataType:caloriesBurned`  | `value`          | `active_calories`    | kcal |
| `health.dataType:activeCalories`* | `value`          | `active_calories`    | kcal |
| `health.dataType:totalSleep`      | `sleepDuration`  | `sleep_seconds`      | s    |
| `health.dataType:asleep`          | `sleepDuration`  | `sleep_seconds`      | s    |
| `health.dataType:deepAsleep`      | `sleepDuration`  | `restful_sleep_seconds` | s |
| `health.dataType:lightAsleep`     | `sleepDuration`  | *(skip — no canonical light-sleep metric)* | — |
| `health.dataType:waterIntake`     | `value`          | `water_ml`           | ml   |
| `health.dataType:foodConsumption`*| `calories` / `protein` / `carbs` / `fat` | `food_kcal` / `protein_g` / `carbs_g` / `fat_g` | kcal / g |
| `health.dataType:exerciseState`   | —                | *(skip; sessions come from events)* | — |
| `health.dataType:respiratoryRate`, `bodyWeight`, `oxygenSaturation`, `temperature`, `vo2Max` | — | *(skip — not in vocabulary)* | — |

\* `activeCalories` and `foodConsumption` are Google-fit-derived types; when
a field is absent, emit nothing (never `0` — zero is a real measurement).

### 3.2 Apple Health (export ZIP → `export.xml`)

Records are `<HKQuantitySample>` / `<HKCategorySample>` elements. Use the
record's `startDate` as `timestamp`.

| Apple Health identifier (element)            | → Canonical metric      | Notes |
| -------------------------------------------- | ----------------------- | ----- |
| `HKQuantityTypeIdentifierHeartRate`          | `heart_rate`            | value in bpm |
| `HKQuantityTypeIdentifierRestingHeartRate`   | `resting_hr`            | |
| `HKQuantityTypeIdentifierStepCount`          | `steps`                 | |
| `HKQuantityTypeIdentifierDistanceWalkingRunning` / `...RunningWalking` | `distance_m` | m |
| `HKQuantityTypeIdentifierAppleActiveBurn`    | `active_calories`       | kcal |
| `HKQuantityTypeIdentifierAppleBasalBurn`     | `resting_kcal`          | kcal |
| `HKQuantityTypeIdentifierAppleExerciseTime`  | `active_seconds`        | s    |
| `HKCategoryTypeIdentifierAppleSleepingWristTime` | `sleep_seconds`     | sum of category values 2 (light) + 3 (deep) + 4 (REM); emit one `sleep_seconds` per night's interval |
| `HKQuantityTypeIdentifierDietaryWater`       | `water_ml`              | ml |
| `HKQuantityTypeIdentifierDietaryCalories`    | `food_kcal`             | kcal |
| `HKQuantityTypeIdentifierDietaryProtein`     | `protein_g`             | g    |
| `HKQuantityTypeIdentifierDietaryCarbohydrates`| `carbs_g`               | g    |
| `HKQuantityTypeIdentifierDietaryFat`         | `fat_g`                 | g    |
| `HKQuantityTypeIdentifierBasalBodyTemperature`, `...EnergyBurned` (total), `...OxygenSaturation`, `...DietaryFiber`, `...InterbeatRecording` | *(skip — not in vocabulary)* | |

### 3.3 Garmin (Connect CSV export)

Connect's CSV layouts vary by export type (Health Summary, Daily Activity,
Training). Match on **column name**, emit one sample per column; use the row
time as `timestamp`.

| Garmin CSV column                              | → Canonical metric      | Unit |
| ---------------------------------------------- | ----------------------- | ---- |
| `Steps`                                        | `steps`                 | count|
| `Distance (mi)` / `Distance (km)`              | `distance_m`            | convert to m |
| `Calories Burned (kcal)` (active column)       | `active_calories`       | kcal |
| `Active Calories (kcal)`                       | `active_calories`       | kcal |
| `Resting Calories (kcal)` / `Resting Calories` | `resting_kcal`          | kcal |
| `Heart Rate (BPM)`                             | `heart_rate`            | bpm  |
| `Resting Heart Rate`                           | `resting_hr`            | bpm  |
| `HRV (ms)`                                     | `hrv`                   | ms   |
| `Sleep` / `Total Sleep` (minutes or h:mm)      | `sleep_seconds`         | s    |
| `Deep Sleep` / `Deep Sleep (h:mm)`             | `restful_sleep_seconds` | s    |
| `Light Sleep`, `REM Sleep`, `Time in Bed`, `Sleep Score` | *(skip — no canonical home)* | — |
| `Intensity (1-5)`                              | `movement_intensity`    | scale to 0–100 |
| `VO2 Max`, `Body Weight (kg)`, `Body Temp (F)`, `Training Effect` | *(skip — not in vocabulary)* | — |

---

## 4. Out of scope

Continuous, native sync (live Health Connect / Apple HealthKit / Garmin
Health API polling) needs a companion app with the required device
permissions. That is **not** part of this POC — the import endpoint is the
batch/export path, and the Pebble watch covers continuous sync via
`/api/v1/health/batch`.
