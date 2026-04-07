use quick_xml::events::BytesStart;
use std::io::BufRead;

use crate::mzml::{
    schema::TagId,
    structs::*,
    utilities::{
        ParamCollector, ParseError, attr, attr_u32, parsing_workspace::ParsingWorkspace,
        read_cv_param, read_user_param,
    },
};

pub(crate) fn parse_sample_list<R: BufRead>(
    ws: &mut ParsingWorkspace<R>,
    start: &BytesStart<'_>,
) -> Result<SampleList, ParseError> {
    let mut list = SampleList {
        count: attr_u32(start, b"count"),
        ..Default::default()
    };
    ws.for_each_child(start, |ws, event| {
        let (tag, element, is_open) = event.into_parts();
        match tag {
            TagId::CvParam => {
                list.receive_cv(read_cv_param(&element));
                Ok(true)
            }
            TagId::UserParam => {
                list.receive_user(read_user_param(&element));
                Ok(true)
            }
            TagId::Sample if is_open => {
                list.samples.push(parse_sample(ws, &element)?);
                Ok(true)
            }
            TagId::Sample => {
                list.samples.push(Sample {
                    id: attr(&element, b"id").unwrap_or_default(),
                    name: attr(&element, b"name").unwrap_or_default(),
                    ..Default::default()
                });
                Ok(true)
            }
            _ => Ok(false),
        }
    })?;
    Ok(list)
}

fn parse_sample<R: BufRead>(
    ws: &mut ParsingWorkspace<R>,
    start: &BytesStart<'_>,
) -> Result<Sample, ParseError> {
    let mut sample = Sample {
        id: attr(start, b"id").unwrap_or_default(),
        name: attr(start, b"name").unwrap_or_default(),
        ..Default::default()
    };
    ws.collect_params_into(start, &mut sample)?;
    Ok(sample)
}
