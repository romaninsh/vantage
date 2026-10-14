use vantage_core::IntoVec;

#[test]
fn vec_passes_through() {
    assert_eq!(vec![1, 2, 3].into_vec(), vec![1, 2, 3]);
}

#[test]
fn array_becomes_vec() {
    assert_eq!([1, 2].into_vec(), vec![1, 2]);
}

#[test]
fn slice_becomes_vec() {
    let items = vec![String::from("a"), String::from("b")];
    assert_eq!(items.as_slice().into_vec(), items);
}

#[test]
fn single_str_becomes_one_element_vec() {
    let v: Vec<String> = "users".into_vec();
    assert_eq!(v, vec!["users".to_string()]);
}

#[test]
fn str_array_becomes_string_vec() {
    let v: Vec<String> = ["a", "b"].into_vec();
    assert_eq!(v, vec!["a".to_string(), "b".to_string()]);
}
