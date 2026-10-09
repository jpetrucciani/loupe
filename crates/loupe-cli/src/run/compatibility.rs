use std::io::Cursor;
use std::sync::Arc;

use loupe_core::backend::Evaluation;
use loupe_core::encoder::Counts;
use loupe_core::input::{InputFormat, Reader};
use loupe_core::policy::Policy;
use loupe_core::wire::{Request, Response};
use serde_json::Value;

use crate::args::Output;
use crate::run::{Completed, Kind, emit};

#[cfg(test)]
mod tests {
    use crate::run::compatibility::*;

    #[test]
    #[allow(clippy::unwrap_used)]
    fn compatibility_actual_response_codecs_and_output_bytes() {
        let capture: Value = serde_json::from_str(include_str!(
            "../../tests/compatibility/v0.1/actual-response.json"
        ))
        .unwrap();
        assert_eq!(
            blake3::hash(include_bytes!(
                "../../../loupe-core/testdata/captured-response.json"
            ))
            .to_hex()
            .as_str(),
            capture["source"]["blake3"].as_str().unwrap()
        );
        let request: Request = serde_json::from_value(capture["request"].clone()).unwrap();
        request.validate().unwrap();
        let response: Response = serde_json::from_value(capture["response"].clone()).unwrap();
        let answers = response.validate(&request).unwrap();
        crate::compatibility::snapshot_json(
            "request.json",
            &serde_json::to_value(&request).unwrap(),
        );
        crate::compatibility::snapshot_json("ask.json", &serde_json::to_value(&response).unwrap());
        let state = request.state.as_str().unwrap();
        let mut reader = Reader::new(
            Cursor::new(state.as_bytes()),
            InputFormat::Whole,
            None,
            true,
        );
        let unit = reader.next_unit().unwrap().unwrap();
        let counts: Counts = serde_json::from_value(capture["counts"].clone()).unwrap();
        let evaluation = Arc::new(Evaluation {
            response,
            cached: false,
            warning: None,
        });
        let request = Arc::new(request);
        let counts = Arc::new(counts);
        for (id, kind, name) in [
            ("outage", Kind::Keep, "noul"),
            ("severity", Kind::Tag, "choice"),
            ("urgency", Kind::Rate, "score"),
        ] {
            let assessment = answers[id].assessment(&Policy::default()).unwrap();
            let completed = Completed {
                unit: unit.clone(),
                request: request.clone(),
                counts: counts.clone(),
                evaluation: Some(evaluation.clone()),
                answer_id: id.to_owned(),
                packed_indices: None,
                split_plan: None,
                view: None,
                focus_plan: None,
                fallback: None,
                ensemble: None,
                review: None,
            };
            for (format, suffix) in [
                (Output::Json, "jsonl"),
                (Output::Tsv, "tsv"),
                (Output::Plain, "plain"),
                (Output::Annotate, "annotate"),
            ] {
                let mut bytes = Vec::new();
                emit(&mut bytes, &completed, &assessment, format, kind, None).unwrap();
                crate::compatibility::snapshot(
                    &format!("{name}.{suffix}"),
                    std::str::from_utf8(&bytes).unwrap(),
                );
            }
        }
    }
}
