-- @endpoint simpleConcatenation
-- @param summand1
-- @param summand2
select @summand1::TEXT || ' <> ' || @summand2::TEXT as result, 'concatenated succesfully' as info
