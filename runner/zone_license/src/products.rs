use zone_enterprise::Plan;

use crate::product::Product;
use crate::products_error::ProductsError;

pub const SEPARATOR: char = ',';

/// Never empty: `parse` refuses a list without a product.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Products(Vec<Product>);

impl Products {
    pub fn parse(list: &str) -> Result<Self, ProductsError> {
        let mut products: Vec<Product> = Vec::new();
        for entry in list
            .split(SEPARATOR)
            .map(str::trim)
            .filter(|entry| !entry.is_empty())
        {
            let product: Product = entry.parse()?;
            if products.iter().any(|listed| listed.id == product.id) {
                return Err(ProductsError::Duplicate {
                    entry: entry.to_owned(),
                    id: product.id,
                });
            }
            products.push(product);
        }
        if products.is_empty() {
            return Err(ProductsError::Empty);
        }
        Ok(Self(products))
    }

    pub fn plan(&self, id: &str) -> Option<Plan> {
        self.0
            .iter()
            .find(|product| product.id == id)
            .map(|product| product.plan)
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

#[cfg(test)]
mod tests;
