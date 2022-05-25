use rand_core::{RngCore, CryptoRng};

use ff::Field;
use group::Group;

use crate::Curve;

#[allow(non_snake_case)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SchnorrSignature<C: Curve> {
  pub R: C::G,
  pub s: C::F,
}

impl<C: Curve> SchnorrSignature<C> {
  pub fn serialize(&self) -> Vec<u8> {
    let mut res = Vec::with_capacity(C::G_len() + C::F_len());
    res.extend(C::G_to_bytes(&self.R));
    res.extend(C::F_to_bytes(&self.s));
    res
  }
}

pub(crate) fn sign<C: Curve>(
  private_key: C::F,
  nonce: C::F,
  challenge: C::F
) -> SchnorrSignature<C> {
  SchnorrSignature {
    R: C::generator_table() * nonce,
    s: nonce + (private_key * challenge)
  }
}

pub(crate) fn verify<C: Curve>(
  public_key: C::G,
  challenge: C::F,
  signature: &SchnorrSignature<C>
) -> bool {
  (C::generator_table() * signature.s) == (signature.R + (public_key * challenge))
}

pub(crate) fn batch_verify<C: Curve, R: RngCore + CryptoRng>(
  rng: &mut R,
  triplets: &[(u16, C::G, C::F, SchnorrSignature<C>)]
) -> Result<(), u16> {
  let mut first = true;
  let mut scalars = Vec::with_capacity(triplets.len() * 3);
  let mut points = Vec::with_capacity(triplets.len() * 3);
  for triple in triplets {
    let mut u = C::F::one();
    if !first {
      u = C::F::random(&mut *rng);
    }

    // uR
    scalars.push(u);
    points.push(triple.3.R);

    // -usG
    scalars.push(-triple.3.s * u);
    points.push(C::generator());

    // ucA
    scalars.push(if first { first = false; triple.2 } else { triple.2 * u});
    points.push(triple.1);
  }

  // s = r + ca
  // sG == R + cA
  // R + cA - sG == 0
  if C::multiexp_vartime(&scalars, &points) == C::G::identity() {
    Ok(())
  } else {
    for triple in triplets {
      if !verify::<C>(triple.1, triple.2, &triple.3) {
        Err(triple.0)?;
      }
    }
    Err(0)
  }
}
