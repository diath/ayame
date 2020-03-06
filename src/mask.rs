pub fn check_mask(mask: &str, value: &str) -> bool {
    /* NOTE(diath): Wildcard expression rules:
        A question mark matches any character exactly one time.
        An asterisk matches any character any number of times.
        A backslash escapes a question mark or an asterisk.
        Any other character is matched literally.
    */

    /* TODO(diath): Add suport for *. */
    let value = value.to_string().into_bytes();
    let mut index = 0 as usize;
    let mut escape = false;

    for chr in mask.chars() {
        if index >= value.len() {
            return false;
        }

        match chr {
            '?' => {
                if escape {
                    if chr != value[index] as char {
                        return false;
                    }

                    escape = false;
                }

                index += 1;
            }
            '\\' => {
                escape = true;
            }
            _ => {
                if chr != value[index] as char {
                    return false;
                }

                index += 1;
            }
        }
    }

    true
}
