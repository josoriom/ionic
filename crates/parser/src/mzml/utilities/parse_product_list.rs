use std::io::BufRead;

use quick_xml::events::BytesStart;

use crate::mzml::{
    schema::TagId,
    structs::*,
    utilities::{
        PREALLOC_CAP, ParamCollector, ParseError, attr, attr_usize, parse_isolation_window,
        parsing_workspace::ParsingWorkspace, read_cv_param, read_user_param,
    },
};

pub(crate) fn parse_product_list<R: BufRead>(
    ws: &mut ParsingWorkspace<R>,
    start: &BytesStart<'_>,
) -> Result<ProductList, ParseError> {
    let count = attr_usize(start, b"count");
    let mut list = ProductList {
        count,
        products: Vec::with_capacity(count.unwrap_or(0).min(PREALLOC_CAP)),
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
            TagId::Product if is_open => {
                list.products.push(parse_product(ws, &element)?);
                Ok(true)
            }
            TagId::Product => {
                list.products.push(Product {
                    spectrum_ref: attr(&element, b"spectrumRef"),
                    source_file_ref: attr(&element, b"sourceFileRef"),
                    external_spectrum_id: attr(&element, b"externalSpectrumID"),
                    ..Default::default()
                });
                Ok(true)
            }
            _ => Ok(false),
        }
    })?;
    Ok(list)
}

pub(crate) fn parse_product<R: BufRead>(
    ws: &mut ParsingWorkspace<R>,
    start: &BytesStart<'_>,
) -> Result<Product, ParseError> {
    let mut product = Product {
        spectrum_ref: attr(start, b"spectrumRef"),
        source_file_ref: attr(start, b"sourceFileRef"),
        external_spectrum_id: attr(start, b"externalSpectrumID"),
        ..Default::default()
    };
    ws.for_each_child(start, |ws, event| {
        let (tag, element, is_open) = event.into_parts();
        match tag {
            TagId::CvParam => {
                product.receive_cv(read_cv_param(&element));
                Ok(true)
            }
            TagId::UserParam => {
                product.receive_user(read_user_param(&element));
                Ok(true)
            }
            TagId::IsolationWindow if is_open => {
                product.isolation_window = Some(parse_isolation_window(ws, &element)?);
                Ok(true)
            }
            _ => Ok(false),
        }
    })?;
    Ok(product)
}
